//! Request capability extraction

use super::Protocol;
use crate::router::types::RequestCapabilities;
use serde_json::Value as JsonValue;

/// Extract required capabilities from request
pub fn extract_capabilities(request: &JsonValue, protocol: Protocol) -> RequestCapabilities {
    RequestCapabilities {
        needs_tools: detect_tools(request, protocol),
        needs_vision: detect_vision(request, protocol),
        needs_streaming: request
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        max_tokens: request
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        modalities: detect_modalities(request, protocol),
    }
}

/// Detect if request needs tool/function support
fn detect_tools(request: &JsonValue, _protocol: Protocol) -> bool {
    request.get("tools").is_some()
        || request.get("functions").is_some()
        || request.get("tool_choice").is_some()
        || request.get("function_call").is_some()
}

/// Detect if request contains image content
fn detect_vision(request: &JsonValue, protocol: Protocol) -> bool {
    match protocol {
        Protocol::OpenAIChat => detect_vision_openai(request),
        Protocol::Anthropic => detect_vision_anthropic(request),
        _ => false,
    }
}

/// Detect vision in OpenAI format
fn detect_vision_openai(request: &JsonValue) -> bool {
    if let Some(messages) = request.get("messages").and_then(|m| m.as_array()) {
        for message in messages {
            if let Some(content) = message.get("content") {
                // Check if content is an array (multi-modal)
                if let Some(content_array) = content.as_array() {
                    for item in content_array {
                        if let Some(item_type) = item.get("type").and_then(|t| t.as_str())
                            && (item_type == "image_url" || item_type == "image")
                        {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Detect vision in Anthropic format
fn detect_vision_anthropic(request: &JsonValue) -> bool {
    if let Some(messages) = request.get("messages").and_then(|m| m.as_array()) {
        for message in messages {
            if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                for block in content {
                    if let Some(block_type) = block.get("type").and_then(|t| t.as_str())
                        && block_type == "image"
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Detect modalities in the request
fn detect_modalities(request: &JsonValue, protocol: Protocol) -> Vec<String> {
    let mut modalities = vec!["text".to_string()];

    if detect_vision(request, protocol) {
        modalities.push("vision".to_string());
    }

    // Check for audio (future support)
    if request.get("audio").is_some() {
        modalities.push("audio".to_string());
    }

    modalities
}
