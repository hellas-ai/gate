//! Inference API client methods

use super::{ClientError, GateClient};
use crate::types::{
    AnthropicMessagesRequest, OpenAIChatCompletionRequest, OpenAICompletionRequest,
};
use serde_json::Value as JsonValue;

impl GateClient {
    /// Send an Anthropic Messages API request
    pub async fn messages(
        &self,
        request: AnthropicMessagesRequest,
    ) -> Result<JsonValue, ClientError> {
        let req = self
            .request(reqwest::Method::POST, "/v1/messages")?
            .json(&request);
        self.execute(req).await
    }

    /// Send an OpenAI Chat Completions API request
    pub async fn chat_completions(
        &self,
        request: OpenAIChatCompletionRequest,
    ) -> Result<JsonValue, ClientError> {
        let req = self
            .request(reqwest::Method::POST, "/v1/chat/completions")?
            .json(&request);
        self.execute(req).await
    }

    /// Send an OpenAI Completions API request (legacy)
    pub async fn completions(
        &self,
        request: OpenAICompletionRequest,
    ) -> Result<JsonValue, ClientError> {
        let req = self
            .request(reqwest::Method::POST, "/v1/completions")?
            .json(&request);
        self.execute(req).await
    }
}
