//! Wrapped client that handles auth errors automatically

use gate_http::client::{
    error::ClientError,
    inference_typed::{
        ChatCompletionRequest, ChatCompletionResponse, MessageRequest, MessageResponse,
        ModelsResponse,
    },
    AuthenticatedGateClient,
};

/// Wrapper around AuthenticatedGateClient that handles auth errors
#[derive(Clone)]
pub struct WrappedAuthClient {
    inner: AuthenticatedGateClient,
}

impl WrappedAuthClient {
    /// Create a new wrapped client
    pub fn new(client: AuthenticatedGateClient) -> Self {
        Self { inner: client }
    }

    /// Execute a request and handle auth errors
    pub async fn execute<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, ClientError> {
        match self.inner.execute(request).await {
            Ok(result) => Ok(result),
            Err(error) => {
                // Check if this is an auth error
                if error.is_auth_expired() {
                    // Trigger the global auth error handler
                    crate::auth::error_handler::trigger_auth_error();
                }
                Err(error)
            }
        }
    }

    /// Create a request builder with authentication
    pub fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.inner.request(method, path)
    }

    /// List available models (requires authentication)
    pub async fn list_models(&self) -> Result<ModelsResponse, ClientError> {
        let request = self.request(reqwest::Method::GET, "/v1/models");
        self.execute(request).await
    }

    /// Create a chat completion (requires authentication)
    pub async fn create_chat_completion(
        &self,
        req: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ClientError> {
        let request = self
            .request(reqwest::Method::POST, "/v1/chat/completions")
            .json(&req);
        self.execute(request).await
    }

    /// Create a message (Anthropic format, requires authentication)
    pub async fn create_message(
        &self,
        req: MessageRequest,
    ) -> Result<MessageResponse, ClientError> {
        let request = self
            .request(reqwest::Method::POST, "/v1/messages")
            .json(&req);
        self.execute(request).await
    }

    /// Get a reference to the inner client (use sparingly - prefer wrapped methods)
    pub fn inner(&self) -> &AuthenticatedGateClient {
        &self.inner
    }
}
