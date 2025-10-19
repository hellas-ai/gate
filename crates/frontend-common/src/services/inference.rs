//! Inference service for communicating with LLM endpoints

use gate_http::client::{error::ClientError, AuthenticatedGateClient};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Message role
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// Chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

/// Chat completion request for OpenAI-compatible endpoints
#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub stream: bool,
}

/// Anthropic message content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

/// Anthropic message format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: Vec<AnthropicContent>,
}

/// Anthropic messages request
#[derive(Debug, Clone, Serialize)]
pub struct AnthropicMessagesRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub stream: bool,
}

/// Provider type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Provider {
    OpenAI,
    Anthropic,
}

/// Model information from the API
#[derive(Debug, Clone, Deserialize)]
pub struct Model {
    pub id: String,
    pub owned_by: String,
}

/// Models list response
#[derive(Debug, Clone, Deserialize)]
pub struct ModelsResponse {
    pub data: Vec<Model>,
}

/// Inference service for making LLM API calls
#[derive(Clone)]
pub struct InferenceService {
    client: AuthenticatedGateClient,
}

impl InferenceService {
    /// Create a new inference service with an authenticated client
    pub fn new(client: AuthenticatedGateClient) -> Self {
        Self { client }
    }

    /// Fetch available models (requires authentication)
    pub async fn get_models(&self) -> Result<Vec<Model>, ClientError> {
        let response = self.client.list_models().await?;
        Ok(response
            .data
            .into_iter()
            .map(|m| Model {
                id: m.id,
                owned_by: m.owned_by,
            })
            .collect())
    }

    /// Send a chat completion request (requires authentication)
    pub async fn chat_completion(
        &self,
        provider: Provider,
        model: String,
        messages: Vec<ChatMessage>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
    ) -> Result<JsonValue, ClientError> {
        match provider {
            Provider::OpenAI => {
                // Build OpenAI-style request
                let messages = messages
                    .into_iter()
                    .map(|msg| gate_http::client::inference::ChatMessage {
                        role: match msg.role {
                            Role::System => "system",
                            Role::User => "user",
                            Role::Assistant => "assistant",
                        }
                        .to_string(),
                        content: msg.content,
                    })
                    .collect();

                let request = gate_http::client::inference::ChatCompletionRequest {
                    model,
                    messages,
                    temperature,
                    max_tokens,
                    stream: Some(false),
                };

                let response = self.client.create_chat_completion(request).await?;
                serde_json::to_value(response).map_err(ClientError::Serialization)
            }
            Provider::Anthropic => {
                // Convert messages to Anthropic format
                let anthropic_messages: Vec<gate_http::client::inference::AnthropicMessage> =
                    messages
                        .into_iter()
                        .filter(|msg| !matches!(msg.role, Role::System)) // Anthropic doesn't use system role in messages
                        .map(|msg| gate_http::client::inference::AnthropicMessage {
                            role: match msg.role {
                                Role::User => "user",
                                Role::Assistant => "assistant",
                                Role::System => unreachable!(),
                            }
                            .to_string(),
                            content: msg.content,
                        })
                        .collect();

                let request = gate_http::client::inference::MessageRequest {
                    model,
                    messages: anthropic_messages,
                    max_tokens: max_tokens.unwrap_or(1024), // Anthropic requires max_tokens
                    temperature,
                    stream: Some(false),
                    system: None, // TODO: Extract system message if present
                };

                let response = self.client.create_message(request).await?;
                serde_json::to_value(response).map_err(ClientError::Serialization)
            }
        }
    }

    /// Parse response based on provider format
    pub fn parse_response(provider: Provider, response: &JsonValue) -> Option<String> {
        match provider {
            Provider::OpenAI => {
                // Parse OpenAI response format
                response
                    .get("choices")?
                    .get(0)?
                    .get("message")?
                    .get("content")?
                    .as_str()
                    .map(|s| s.to_string())
            }
            Provider::Anthropic => {
                // Parse Anthropic response format
                response
                    .get("content")?
                    .get(0)?
                    .get("text")?
                    .as_str()
                    .map(|s| s.to_string())
            }
        }
    }

    /// Detect provider from model name
    pub fn detect_provider(model: &str) -> Provider {
        if model.starts_with("claude") {
            Provider::Anthropic
        } else {
            Provider::OpenAI
        }
    }
}
