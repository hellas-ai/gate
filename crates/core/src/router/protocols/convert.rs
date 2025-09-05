//! Protocol conversion implementations

use super::Protocol;
use crate::Result;
use serde_json::{Value as JsonValue, json};

/// Check if conversion between protocols is possible
pub fn can_convert(from: Protocol, to: Protocol) -> bool {
    match (from, to) {
        (Protocol::Anthropic, Protocol::OpenAIChat) => true,
        (Protocol::OpenAIChat, Protocol::Anthropic) => true,
        (Protocol::OpenAICompletions, Protocol::OpenAIChat) => true,
        (a, b) if a == b => true,
        _ => false,
    }
}

/// Get expected information loss for a conversion
pub fn conversion_loss(from: Protocol, to: Protocol) -> Vec<String> {
    match (from, to) {
        (Protocol::Anthropic, Protocol::OpenAIChat) => vec![
            "system prompts handled differently".to_string(),
            "cache_control not supported".to_string(),
        ],
        (Protocol::OpenAIChat, Protocol::Anthropic) => vec![
            "function_call becomes tool_use".to_string(),
            "logprobs not supported".to_string(),
        ],
        (Protocol::OpenAICompletions, Protocol::OpenAIChat) => {
            vec!["completion context becomes single user message".to_string()]
        }
        _ => vec![],
    }
}

/// Convert request between protocols
pub fn convert_request(
    from: Protocol,
    to: Protocol,
    json: &JsonValue,
) -> Result<(JsonValue, Vec<String>)> {
    match (from, to) {
        (Protocol::Anthropic, Protocol::OpenAIChat) => anthropic_to_openai_chat(json),
        (Protocol::OpenAIChat, Protocol::Anthropic) => openai_chat_to_anthropic(json),
        (Protocol::OpenAICompletions, Protocol::OpenAIChat) => completions_to_chat(json),
        (a, b) if a == b => Ok((json.clone(), vec![])),
        _ => Err(crate::Error::UnsupportedConversion(
            format!("{from:?}"),
            format!("{to:?}"),
        )),
    }
}

/// Convert response between protocols
pub fn convert_response(
    from: Protocol,
    to: Protocol,
    json: &JsonValue,
) -> Result<(JsonValue, Vec<String>)> {
    match (from, to) {
        (Protocol::OpenAIChat, Protocol::Anthropic) => openai_chat_response_to_anthropic(json),
        (Protocol::Anthropic, Protocol::OpenAIChat) => anthropic_response_to_openai_chat(json),
        (a, b) if a == b => Ok((json.clone(), vec![])),
        _ => Err(crate::Error::UnsupportedConversion(
            format!("{from:?}"),
            format!("{to:?}"),
        )),
    }
}

/// Convert Anthropic request to OpenAI chat format
fn anthropic_to_openai_chat(json: &JsonValue) -> Result<(JsonValue, Vec<String>)> {
    let mut warnings = Vec::new();
    let mut result = json!({});

    // Copy model
    if let Some(model) = json.get("model") {
        result["model"] = model.clone();
    }

    // Convert messages
    if let Some(messages) = json.get("messages") {
        let mut converted_messages = Vec::new();

        // Add system message if present
        if let Some(system) = json.get("system") {
            converted_messages.push(json!({
                "role": "system",
                "content": system
            }));
        }

        // Convert each message
        if let Some(msg_array) = messages.as_array() {
            for msg in msg_array {
                let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
                let content = msg.get("content");

                // Handle content blocks (Anthropic can have multiple content blocks)
                if let Some(content_array) = content.and_then(|c| c.as_array()) {
                    let mut text_parts = Vec::new();
                    for block in content_array {
                        if let Some(block_type) = block.get("type").and_then(|t| t.as_str()) {
                            match block_type {
                                "text" => {
                                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                        text_parts.push(text.to_string());
                                    }
                                }
                                "image" => {
                                    warnings.push("Image content not fully supported".to_string());
                                }
                                _ => {
                                    warnings
                                        .push(format!("Content type {block_type} not supported"));
                                }
                            }
                        }
                    }
                    converted_messages.push(json!({
                        "role": role,
                        "content": text_parts.join("\n")
                    }));
                } else if let Some(content_str) = content.and_then(|c| c.as_str()) {
                    converted_messages.push(json!({
                        "role": role,
                        "content": content_str
                    }));
                }
            }
        }

        result["messages"] = json!(converted_messages);
    }

    // Copy common parameters
    for field in &["temperature", "max_tokens", "top_p", "stream", "stop"] {
        if let Some(value) = json.get(*field) {
            result[*field] = value.clone();
        }
    }

    // Handle tools
    if let Some(tools) = json.get("tools") {
        result["tools"] = tools.clone();
        if let Some(tool_choice) = json.get("tool_choice") {
            result["tool_choice"] = tool_choice.clone();
        }
    }

    // Warn about unsupported fields
    if json.get("cache_control").is_some() {
        warnings.push("cache_control not supported in OpenAI format".to_string());
    }

    Ok((result, warnings))
}

/// Convert OpenAI chat to Anthropic format
fn openai_chat_to_anthropic(json: &JsonValue) -> Result<(JsonValue, Vec<String>)> {
    let mut warnings = Vec::new();
    let mut result = json!({
        "anthropic_version": "2024-10-22"
    });

    // Copy model
    if let Some(model) = json.get("model") {
        result["model"] = model.clone();
    }

    // Convert messages
    if let Some(messages) = json.get("messages").and_then(|m| m.as_array()) {
        let mut system_content = None;
        let mut converted_messages = Vec::new();

        for msg in messages {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let content = msg.get("content");

            match role {
                "system" => {
                    // Anthropic uses a separate system field
                    if let Some(content_str) = content.and_then(|c| c.as_str()) {
                        system_content = Some(content_str.to_string());
                    }
                }
                "assistant" => {
                    if let Some(content_str) = content.and_then(|c| c.as_str()) {
                        converted_messages.push(json!({
                            "role": "assistant",
                            "content": [{
                                "type": "text",
                                "text": content_str
                            }]
                        }));
                    }
                }
                "user" => {
                    if let Some(content_str) = content.and_then(|c| c.as_str()) {
                        converted_messages.push(json!({
                            "role": "user",
                            "content": [{
                                "type": "text",
                                "text": content_str
                            }]
                        }));
                    }
                }
                _ => {
                    warnings.push(format!("Unknown role: {role}"));
                }
            }
        }

        if let Some(system) = system_content {
            result["system"] = json!(system);
        }
        result["messages"] = json!(converted_messages);
    }

    // Copy common parameters
    for field in &[
        "temperature",
        "max_tokens",
        "top_p",
        "stream",
        "stop_sequences",
    ] {
        if let Some(value) = json.get(*field) {
            result[*field] = value.clone();
        }
    }

    // Handle tools/functions
    if let Some(tools) = json.get("tools") {
        result["tools"] = tools.clone();
        if let Some(tool_choice) = json.get("tool_choice") {
            result["tool_choice"] = tool_choice.clone();
        }
    } else if let Some(functions) = json.get("functions") {
        // Convert functions to tools format
        warnings.push("functions converted to tools format".to_string());
        let mut tools = Vec::new();
        if let Some(func_array) = functions.as_array() {
            for func in func_array {
                tools.push(json!({
                    "type": "function",
                    "function": func
                }));
            }
        }
        result["tools"] = json!(tools);
    }

    // Warn about unsupported fields
    if json.get("logprobs").is_some() {
        warnings.push("logprobs not supported in Anthropic format".to_string());
    }
    if json.get("n").is_some() && json.get("n") != Some(&json!(1)) {
        warnings.push("n>1 not supported in Anthropic format".to_string());
    }

    Ok((result, warnings))
}

/// Convert OpenAI completions to chat format
fn completions_to_chat(json: &JsonValue) -> Result<(JsonValue, Vec<String>)> {
    let mut warnings = Vec::new();
    let mut result = json!({});

    // Copy model
    if let Some(model) = json.get("model") {
        result["model"] = model.clone();
    }

    // Convert prompt to messages
    if let Some(prompt) = json.get("prompt") {
        let content = if let Some(prompt_str) = prompt.as_str() {
            prompt_str.to_string()
        } else if let Some(prompt_array) = prompt.as_array() {
            prompt_array
                .iter()
                .filter_map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            warnings.push("Complex prompt format not fully supported".to_string());
            "".to_string()
        };

        result["messages"] = json!([{
            "role": "user",
            "content": content
        }]);
    }

    // Copy common parameters
    for field in &[
        "temperature",
        "max_tokens",
        "top_p",
        "stream",
        "stop",
        "presence_penalty",
        "frequency_penalty",
    ] {
        if let Some(value) = json.get(*field) {
            result[*field] = value.clone();
        }
    }

    // Warn about completion-specific fields
    if json.get("suffix").is_some() {
        warnings.push("suffix not supported in chat format".to_string());
    }
    if json.get("echo").is_some() {
        warnings.push("echo not supported in chat format".to_string());
    }

    Ok((result, warnings))
}

/// Convert OpenAI chat response to Anthropic format
fn openai_chat_response_to_anthropic(json: &JsonValue) -> Result<(JsonValue, Vec<String>)> {
    let mut warnings = Vec::new();
    let mut result = json!({
        "type": "message"
    });

    // Copy ID
    if let Some(id) = json.get("id") {
        result["id"] = id.clone();
    }

    // Copy model
    if let Some(model) = json.get("model") {
        result["model"] = model.clone();
    }

    // Convert choices to content
    if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
        if let Some(first_choice) = choices.first() {
            if let Some(message) = first_choice.get("message") {
                let role = message
                    .get("role")
                    .and_then(|r| r.as_str())
                    .unwrap_or("assistant");
                result["role"] = json!(role);

                if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                    result["content"] = json!([{
                        "type": "text",
                        "text": content
                    }]);
                }

                // Handle tool calls
                if let Some(tool_calls) = message.get("tool_calls") {
                    result["tool_calls"] = tool_calls.clone();
                }
            }

            // Copy finish reason
            if let Some(finish_reason) = first_choice.get("finish_reason") {
                result["stop_reason"] = match finish_reason.as_str() {
                    Some("stop") => json!("end_turn"),
                    Some("length") => json!("max_tokens"),
                    Some("tool_calls") => json!("tool_use"),
                    _ => finish_reason.clone(),
                };
            }
        }

        if choices.len() > 1 {
            warnings.push("Multiple choices not supported in Anthropic format".to_string());
        }
    }

    // Convert usage
    if let Some(usage) = json.get("usage") {
        result["usage"] = json!({
            "input_tokens": usage.get("prompt_tokens"),
            "output_tokens": usage.get("completion_tokens")
        });
    }

    Ok((result, warnings))
}

/// Convert Anthropic response to OpenAI chat format
fn anthropic_response_to_openai_chat(json: &JsonValue) -> Result<(JsonValue, Vec<String>)> {
    let warnings = Vec::new();
    let mut result = json!({
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp()
    });

    // Copy ID
    if let Some(id) = json.get("id") {
        result["id"] = id.clone();
    } else {
        result["id"] = json!(format!("chatcmpl-{}", uuid::Uuid::new_v4()));
    }

    // Copy model
    if let Some(model) = json.get("model") {
        result["model"] = model.clone();
    }

    // Convert content to choices
    let mut message = json!({
        "role": json.get("role").unwrap_or(&json!("assistant"))
    });

    if let Some(content) = json.get("content").and_then(|c| c.as_array()) {
        let mut text_parts = Vec::new();
        for block in content {
            if let Some(block_type) = block.get("type").and_then(|t| t.as_str())
                && block_type == "text"
                && let Some(text) = block.get("text").and_then(|t| t.as_str())
            {
                text_parts.push(text.to_string());
            }
        }
        message["content"] = json!(text_parts.join("\n"));
    } else if let Some(content_str) = json.get("content").and_then(|c| c.as_str()) {
        message["content"] = json!(content_str);
    }

    // Handle tool use
    if let Some(tool_calls) = json.get("tool_calls") {
        message["tool_calls"] = tool_calls.clone();
    }

    // Convert stop reason to finish reason
    let finish_reason = if let Some(stop_reason) = json.get("stop_reason").and_then(|s| s.as_str())
    {
        match stop_reason {
            "end_turn" => "stop",
            "max_tokens" => "length",
            "tool_use" => "tool_calls",
            _ => stop_reason,
        }
    } else {
        "stop"
    };

    result["choices"] = json!([{
        "index": 0,
        "message": message,
        "finish_reason": finish_reason
    }]);

    // Convert usage
    if let Some(usage) = json.get("usage") {
        result["usage"] = json!({
            "prompt_tokens": usage.get("input_tokens"),
            "completion_tokens": usage.get("output_tokens"),
            "total_tokens": usage.get("input_tokens").and_then(|i| i.as_u64()).unwrap_or(0)
                + usage.get("output_tokens").and_then(|o| o.as_u64()).unwrap_or(0)
        });
    }

    Ok((result, warnings))
}
