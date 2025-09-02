//! Helper to convert ResponseStream to axum Response

use crate::error::HttpError;
use axum::response::{IntoResponse, Response, Sse, sse::Event};
use futures::stream::StreamExt;
use gate_core::router::{ResponseChunk, ResponseStream};

/// Convert a ResponseStream to an axum Response (SSE stream)
pub async fn response_stream_to_axum(stream: ResponseStream) -> Result<Response, HttpError> {
    let sse_stream = stream.map(|result| -> Result<Event, std::convert::Infallible> {
        match result {
            Ok(chunk) => match chunk {
                ResponseChunk::Content(json) => Ok(Event::default().data(json.to_string())),
                ResponseChunk::Stop {
                    reason,
                    error,
                    cost,
                } => {
                    let mut data = serde_json::json!({
                        "done": true,
                        "reason": format!("{:?}", reason)
                    });

                    if let Some(err) = error {
                        data["error"] = serde_json::json!(err);
                    }

                    if let Some(c) = cost {
                        data["cost"] = serde_json::json!(c);
                    }

                    Ok(Event::default().data(data.to_string()))
                }
                ResponseChunk::Headers(headers) => Ok(Event::default().data(
                    serde_json::json!({
                        "headers": headers
                    })
                    .to_string(),
                )),
                ResponseChunk::Usage {
                    prompt_tokens,
                    completion_tokens,
                } => Ok(Event::default().data(
                    serde_json::json!({
                        "usage": {
                            "prompt_tokens": prompt_tokens,
                            "completion_tokens": completion_tokens
                        }
                    })
                    .to_string(),
                )),
                ResponseChunk::Metadata(metadata) => Ok(Event::default().data(
                    serde_json::json!({
                        "metadata": metadata
                    })
                    .to_string(),
                )),
            },
            Err(e) => Ok(Event::default().data(
                serde_json::json!({
                    "error": e.to_string()
                })
                .to_string(),
            )),
        }
    });

    Ok(Sse::new(sse_stream).into_response())
}
