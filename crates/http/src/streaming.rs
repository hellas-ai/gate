//! SSE streaming helpers for converting ResponseChunks to SSE events

use axum::response::sse::Event;
use gate_core::router::prelude::{Protocol, ResponseChunk};
use serde_json::{Value as JsonValue, json};

/// Maps a ResponseChunk to an SSE event payload based on the protocol
pub fn map_to_sse_event(chunk: ResponseChunk, protocol: Protocol) -> Event {
    let payload = chunk_to_payload(chunk, protocol);
    let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into());
    Event::default().data(data)
}

/// Converts a ResponseChunk to a JSON payload based on the protocol
fn chunk_to_payload(chunk: ResponseChunk, protocol: Protocol) -> JsonValue {
    match chunk {
        ResponseChunk::Content(v) => v,
        ResponseChunk::Usage {
            prompt_tokens,
            completion_tokens,
        } => match protocol {
            Protocol::OpenAIChat | Protocol::OpenAIResponses => json!({
                "usage": {
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens,
                    "total_tokens": (prompt_tokens + completion_tokens)
                }
            }),
            Protocol::Anthropic => json!({
                "usage": {
                    "input_tokens": prompt_tokens,
                    "output_tokens": completion_tokens
                }
            }),
            _ => json!({
                "usage": {
                    "prompt_tokens": prompt_tokens,
                    "completion_tokens": completion_tokens
                }
            }),
        },
        ResponseChunk::Stop {
            error: Some(err), ..
        } => {
            // Represent an error event; clients can handle and close
            json!({"error": err})
        }
        ResponseChunk::Stop { .. } => {
            // Minimal stop signal
            json!({"stop": true})
        }
        ResponseChunk::Headers(_) | ResponseChunk::Metadata(_) => json!({}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gate_core::router::types::StopReason;
    use std::collections::HashMap;

    #[test]
    fn test_content_chunk_to_payload() {
        let content = json!({"text": "Hello"});
        let chunk = ResponseChunk::Content(content.clone());
        let payload = chunk_to_payload(chunk, Protocol::OpenAIChat);
        assert_eq!(payload, content);
    }

    #[test]
    fn test_usage_chunk_openai() {
        let chunk = ResponseChunk::Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
        };
        let payload = chunk_to_payload(chunk, Protocol::OpenAIChat);
        assert_eq!(payload["usage"]["prompt_tokens"], 10);
        assert_eq!(payload["usage"]["completion_tokens"], 20);
        assert_eq!(payload["usage"]["total_tokens"], 30);
    }

    #[test]
    fn test_usage_chunk_anthropic() {
        let chunk = ResponseChunk::Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
        };
        let payload = chunk_to_payload(chunk, Protocol::Anthropic);
        assert_eq!(payload["usage"]["input_tokens"], 10);
        assert_eq!(payload["usage"]["output_tokens"], 20);
    }

    #[test]
    fn test_stop_chunk_with_error() {
        let chunk = ResponseChunk::Stop {
            reason: StopReason::Error,
            error: Some("Test error".to_string()),
            cost: None,
        };
        let payload = chunk_to_payload(chunk, Protocol::OpenAIChat);
        assert_eq!(payload["error"], "Test error");
    }

    #[test]
    fn test_stop_chunk_without_error() {
        let chunk = ResponseChunk::Stop {
            reason: StopReason::Complete,
            error: None,
            cost: None,
        };
        let payload = chunk_to_payload(chunk, Protocol::OpenAIChat);
        assert_eq!(payload["stop"], true);
    }

    #[test]
    fn test_headers_and_metadata_chunks() {
        let headers = ResponseChunk::Headers(HashMap::new());
        let payload = chunk_to_payload(headers, Protocol::OpenAIChat);
        assert_eq!(payload, json!({}));

        let metadata = ResponseChunk::Metadata(HashMap::new());
        let payload = chunk_to_payload(metadata, Protocol::OpenAIChat);
        assert_eq!(payload, json!({}));
    }

    #[test]
    fn test_map_to_sse_event_creates_event() {
        // Just test that the function creates an Event without errors
        let chunk = ResponseChunk::Content(json!({"test": "data"}));
        let _event = map_to_sse_event(chunk, Protocol::OpenAIChat);
        // Event created successfully
    }
}
