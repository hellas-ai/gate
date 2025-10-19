//! Helper to convert ResponseStream to axum Response

use crate::error::HttpError;
use axum::response::Json;
use axum::response::{IntoResponse, Response, Sse, sse::Event};
use futures::stream::{StreamExt, iter};
use gate_core::router::types::ActualCost;
use gate_core::router::{ResponseChunk, ResponseStream};
use http::header::HeaderName;
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
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
                            if obj.get(JSON_TYPE_FIELD).is_some() {
                                to_json_string(&TypeFirst { obj })
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
            Ok(ResponseChunk::Stop {
                error: Some(err), ..
            }) => {
                return Err(HttpError::ServiceUnavailable(err));
            }
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

struct TypeFirst<'a> {
    obj: &'a serde_json::Map<String, JsonValue>,
}

impl<'a> Serialize for TypeFirst<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let len = self.obj.len();
        let mut map = serializer.serialize_map(Some(len))?;
        if let Some(typ) = self.obj.get(JSON_TYPE_FIELD) {
            map.serialize_entry(JSON_TYPE_FIELD, typ)?;
        }
        for (k, v) in self.obj.iter() {
            if k != JSON_TYPE_FIELD {
                map.serialize_entry(k, v)?;
            }
        }
        map.end()
    }
}
