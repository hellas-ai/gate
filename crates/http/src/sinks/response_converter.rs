//! Helper to convert ResponseStream to axum Response

use crate::error::HttpError;
use axum::response::Json;
use axum::response::{IntoResponse, Response, Sse, sse::Event};
use futures::stream::{StreamExt, iter};
use gate_core::router::types::ActualCost;
use gate_core::router::{ResponseChunk, ResponseStream};
use http::header::HeaderName;
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::HashMap;

const JSON_TYPE_FIELD: &str = "type";

/// Convert a ResponseStream to an axum Response (SSE stream)
pub async fn response_stream_to_axum(stream: ResponseStream) -> Result<Response, HttpError> {
    // Peek the first chunk to extract response headers for the HTTP response
    let mut stream = stream;
    let head = stream.next().await;
    let mut response_headers: Option<HashMap<String, String>> = None;
    let head_item = match head {
        Some(Ok(ResponseChunk::Headers(h))) => {
            response_headers = Some(h);
            None
        }
        other => other,
    };

    let head_stream = iter(head_item.into_iter());
    let sse_stream = head_stream
        .chain(stream)
        .filter(|item| {
            // Drop any Headers chunks that may appear later
            let keep = !matches!(item, Ok(ResponseChunk::Headers(_)));
            std::future::ready(keep)
        })
        .map(|result| -> Result<Event, std::convert::Infallible> {
            match result {
                Ok(chunk) => match chunk {
                    // Preserve SSE event names by using payload's "type" field as the SSE event name.
                    ResponseChunk::Content(json) => {
                        let mut ev = Event::default();
                        if let Some(event_name) = json
                            .as_object()
                            .and_then(|o| o.get(JSON_TYPE_FIELD))
                            .and_then(|v| v.as_str())
                        {
                            ev = ev.event(event_name);
                        }
                        // Ensure the "type" field appears first in the serialized JSON for consistency
                        let body = if let Some(obj) = json.as_object() {
                            if let Some(typ) = obj.get(JSON_TYPE_FIELD) {
                                let mut parts = Vec::with_capacity(obj.len());
                                parts.push(format!("\"{JSON_TYPE_FIELD}\":{typ}"));
                                for (k, v) in obj {
                                    if k != JSON_TYPE_FIELD {
                                        // Serialize the key properly to ensure escaping
                                        let key =
                                            serde_json::to_string(k).unwrap_or_else(|_| k.clone());
                                        parts.push(format!("{key}:{v}"));
                                    }
                                }
                                format!("{{{}}}", parts.join(","))
                            } else {
                                json.to_string()
                            }
                        } else {
                            json.to_string()
                        };
                        Ok(ev.data(body))
                    }
                    ResponseChunk::Stop {
                        reason,
                        error,
                        cost,
                    } => {
                        let data = DoneEvent {
                            done: true,
                            reason: format!("{reason:?}"),
                            error,
                            cost,
                        };
                        Ok(Event::default().data(to_json_string(&data)))
                    }
                    ResponseChunk::Headers(headers) => {
                        let data = HeadersEvent { headers };
                        Ok(Event::default().data(to_json_string(&data)))
                    }
                    ResponseChunk::Usage {
                        prompt_tokens,
                        completion_tokens,
                    } => {
                        let data = UsageEvent {
                            usage: UsagePayload {
                                prompt_tokens,
                                completion_tokens,
                            },
                        };
                        Ok(Event::default().data(to_json_string(&data)))
                    }
                    ResponseChunk::Metadata(metadata) => {
                        let data = MetadataEvent { metadata };
                        Ok(Event::default().data(to_json_string(&data)))
                    }
                },
                Err(e) => {
                    let data = ErrorEvent {
                        error: e.to_string(),
                    };
                    Ok(Event::default().data(to_json_string(&data)))
                }
            }
        });
    let mut resp = Sse::new(sse_stream).into_response();
    if let Some(hdrs) = response_headers {
        let headers = resp.headers_mut();
        for (k, v) in hdrs {
            if let (Ok(name), Ok(value)) = (HeaderName::try_from(k), v.parse()) {
                headers.insert(name, value);
            }
        }
    }
    Ok(resp)
}

/// Convert a ResponseStream representing a non-streaming response to a JSON HTTP response
pub async fn response_stream_to_json(stream: ResponseStream) -> Result<Response, HttpError> {
    let mut stream = stream;
    let head = stream.next().await;
    let mut response_headers: Option<HashMap<String, String>> = None;
    let mut last_json: Option<serde_json::Value> = None;

    // Process head
    if let Some(item) = head {
        match item {
            Ok(ResponseChunk::Headers(h)) => {
                response_headers = Some(h);
            }
            Ok(ResponseChunk::Content(json)) => last_json = Some(json),
            Ok(_) => {}
            Err(e) => return Err(HttpError::Core(e)),
        }
    }

    // Process remainder
    while let Some(item) = stream.next().await {
        match item {
            Ok(ResponseChunk::Headers(_)) => { /* ignore duplicates */ }
            Ok(ResponseChunk::Content(json)) => last_json = Some(json),
            Ok(ResponseChunk::Stop {
                error: Some(err), ..
            }) => {
                if last_json.is_none() {
                    return Err(HttpError::ServiceUnavailable(err));
                }
            }
            Ok(_) => {}
            Err(e) => return Err(HttpError::Core(e)),
        }
    }

    if let Some(json) = last_json {
        let mut resp = Json(json).into_response();
        if let Some(hdrs) = response_headers {
            let headers = resp.headers_mut();
            for (k, v) in hdrs {
                if let (Ok(name), Ok(value)) = (HeaderName::try_from(k), v.parse()) {
                    headers.insert(name, value);
                }
            }
        }
        Ok(resp)
    } else {
        Err(HttpError::ServiceUnavailable(
            "No content received from sink".to_string(),
        ))
    }
}

#[derive(Serialize)]
struct DoneEvent {
    done: bool,
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost: Option<ActualCost>,
}

#[derive(Serialize)]
struct HeadersEvent {
    headers: HashMap<String, String>,
}

#[derive(Serialize)]
struct UsageEvent {
    usage: UsagePayload,
}

#[derive(Serialize)]
struct UsagePayload {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[derive(Serialize)]
struct MetadataEvent {
    metadata: HashMap<String, JsonValue>,
}

#[derive(Serialize)]
struct ErrorEvent {
    error: String,
}

fn to_json_string<T: Serialize>(val: &T) -> String {
    serde_json::to_string(val).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use http::header::CONTENT_TYPE;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn test_response_stream_to_json_forwards_headers() {
        let mut hdrs = std::collections::HashMap::new();
        hdrs.insert("request-id".to_string(), "abc123".to_string());
        let json = serde_json::json!({"ok": true});
        let chunks = vec![
            Ok(ResponseChunk::Headers(hdrs)),
            Ok(ResponseChunk::Content(json.clone())),
            Ok(ResponseChunk::Stop {
                reason: gate_core::router::types::StopReason::Complete,
                error: None,
                cost: None,
            }),
        ];
        let stream = Box::pin(stream::iter(chunks));
        let resp = response_stream_to_json(stream).await.expect("json resp");
        assert_eq!(resp.headers().get("request-id").unwrap(), "abc123");
        let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body, json);
    }

    #[tokio::test]
    async fn test_response_stream_to_axum_sets_event_name() {
        let mut hdrs = std::collections::HashMap::new();
        hdrs.insert("x-test".to_string(), "1".to_string());
        let json = serde_json::json!({"type": "message_start", "message": {"id": "m"}});
        let chunks = vec![
            Ok(ResponseChunk::Headers(hdrs)),
            Ok(ResponseChunk::Content(json)),
            Ok(ResponseChunk::Stop {
                reason: gate_core::router::types::StopReason::Complete,
                error: None,
                cost: None,
            }),
        ];
        let stream = Box::pin(stream::iter(chunks));
        let resp = response_stream_to_axum(stream).await.expect("sse resp");
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );
        let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let s = String::from_utf8_lossy(&body_bytes);
        assert!(s.contains("event: message_start"));
        assert!(s.contains("data: {\"type\":\"message_start\""));
    }
}
