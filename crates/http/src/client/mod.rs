//! Gate HTTP client

pub mod auth;
pub mod auth_typed;
pub mod config;
pub mod error;
pub mod inference;
pub mod inference_typed;
pub mod typed;

use error::ClientError;
use reqwest::{Client, ClientBuilder, header};
use std::time::Duration;

pub use typed::{AuthenticatedGateClient, PublicGateClient, TypedClientBuilder};

/// Gate API client
#[derive(Clone)]
pub struct GateClient {
    client: Client,
    base_url: String,
    api_key: Option<String>,
}

impl GateClient {
    /// Create a new client with default configuration
    pub fn new(base_url: impl Into<String>) -> Result<Self, ClientError> {
        Self::builder().base_url(base_url).build()
    }

    /// Create a new client builder
    pub fn builder() -> GateClientBuilder {
        GateClientBuilder::default()
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Create a request builder with authentication
    pub fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let mut request = self.client.request(method, url);

        if let Some(api_key) = &self.api_key {
            request = request.header(header::AUTHORIZATION, format!("Bearer {api_key}"));
        }

        request
    }

    /// Execute a request and handle common errors
    pub async fn execute<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, ClientError> {
        let response = request.send().await?;
        let status = response.status();

        if status.is_success() {
            Ok(response.json().await?)
        } else {
            let message = response.text().await.unwrap_or_else(|_| status.to_string());
            Err(ClientError::from_status(status, message))
        }
    }
}

/// Builder for GateClient
#[derive(Default)]
pub struct GateClientBuilder {
    base_url: Option<String>,
    api_key: Option<String>,
    timeout: Option<Duration>,
    user_agent: Option<String>,
}

impl GateClientBuilder {
    /// Set the base URL
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Set the API key for authentication
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Set the request timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set the user agent
    pub fn user_agent(mut self, agent: impl Into<String>) -> Self {
        self.user_agent = Some(agent.into());
        self
    }

    /// Build the client
    pub fn build(self) -> Result<GateClient, ClientError> {
        let base_url = self
            .base_url
            .ok_or_else(|| ClientError::Configuration("base_url is required".into()))?;

        // Ensure base_url ends without a trailing slash
        let base_url = base_url.trim_end_matches('/').to_string();

        let mut client_builder = ClientBuilder::new();

        #[cfg(not(target_arch = "wasm32"))]
        if let Some(timeout) = self.timeout {
            client_builder = client_builder.timeout(timeout);
        }

        if let Some(user_agent) = self.user_agent {
            client_builder = client_builder.user_agent(user_agent);
        } else {
            client_builder = client_builder.user_agent("gate-client/0.1.0");
        }

        let client = client_builder.build()?;

        Ok(GateClient {
            client,
            base_url,
            api_key: self.api_key,
        })
    }
}
