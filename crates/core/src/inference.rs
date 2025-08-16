use crate::Result;
use async_trait::async_trait;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stream: Option<bool>,
    pub stop: Option<Vec<String>>,
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub logit_bias: Option<Value>,
    pub user: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChatChunkChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChunkChoice {
    pub index: u32,
    pub delta: ChatDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatDelta {
    pub content: Option<String>,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessageRequest {
    pub model: String,
    pub messages: Vec<AnthropicMessage>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub stream: Option<bool>,
    pub stop_sequences: Option<Vec<String>>,
    pub system: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessageResponse {
    pub id: String,
    pub model: String,
    pub role: String,
    pub content: Vec<AnthropicContent>,
    pub stop_reason: Option<String>,
    pub stop_sequence: Option<String>,
    pub usage: AnthropicUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModel {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub path: Option<String>,
    pub context_length: u32,
    pub supports_chat: bool,
    pub supports_completion: bool,
}

pub type ChatCompletionStream = Pin<Box<dyn Stream<Item = Result<ChatCompletionChunk>> + Send>>;
pub type AnthropicMessageStream = Pin<Box<dyn Stream<Item = Result<Value>> + Send>>;

#[async_trait]
pub trait InferenceBackend: Send + Sync {
    async fn chat_completions(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse>;

    async fn chat_completions_stream(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionStream>;

    async fn messages(&self, request: AnthropicMessageRequest) -> Result<AnthropicMessageResponse>;

    async fn messages_stream(
        &self,
        request: AnthropicMessageRequest,
    ) -> Result<AnthropicMessageStream>;

    async fn list_models(&self) -> Result<Vec<LocalModel>>;

    async fn get_model(&self, model_id: &str) -> Result<Option<LocalModel>>;

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}
