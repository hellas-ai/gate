//! Type-safe API clients that enforce authentication requirements at compile time

use super::ClientError;
use reqwest::{Client, ClientBuilder, Method, RequestBuilder, header};
use std::time::Duration;
use url::Url;

/// Client for public endpoints that don't require authentication
#[derive(Clone)]
pub struct PublicGateClient {
    client: Client,
    base_url: Url,
}

/// Client for authenticated endpoints that require a valid API key
#[derive(Clone)]
pub struct AuthenticatedGateClient {
    client: Client,
    base_url: Url,
    api_key: String,
}

impl PublicGateClient {
    /// Create a new public client
    pub fn new(base_url: impl Into<String>) -> Result<Self, ClientError> {
        Self::new_with_timeout(base_url, None)
    }

    /// Create a new public client with optional timeout
    fn new_with_timeout(
        base_url: impl Into<String>,
        timeout: Option<Duration>,
    ) -> Result<Self, ClientError> {
        let mut base_url = Url::parse(&base_url.into())
            .map_err(|e| ClientError::Configuration(format!("invalid base_url: {e}")))?;
        if base_url.path() != "/" && base_url.path().ends_with('/') {
            base_url = base_url
                .join(".")
                .map_err(|e| ClientError::Configuration(format!("invalid base_url path: {e}")))?;
        }

        #[cfg(not(target_arch = "wasm32"))]
        let client = {
            let mut builder = ClientBuilder::new().user_agent("gate-client/0.1.0");
            if let Some(timeout) = timeout {
                builder = builder.timeout(timeout);
            }
            builder.build()?
        };

        #[cfg(target_arch = "wasm32")]
        let client = {
            let _ = timeout; // Timeouts not supported on WASM
            ClientBuilder::new()
                .user_agent("gate-client/0.1.0")
                .build()?
        };

        Ok(Self { client, base_url })
    }

    /// Get the base URL
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Create a request builder without authentication
    pub fn request(&self, method: Method, path: &str) -> Result<RequestBuilder, ClientError> {
        let url = self
            .base_url
            .join(path.trim_start_matches('/'))
            .map_err(|e| ClientError::Configuration(format!("invalid path '{path}': {e}")))?;
        Ok(self.client.request(method, url))
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

    /// Authenticate with an API key to get an authenticated client
    pub fn authenticate(self, api_key: impl Into<String>) -> AuthenticatedGateClient {
        AuthenticatedGateClient {
            client: self.client,
            base_url: self.base_url,
            api_key: api_key.into(),
        }
    }
}

impl AuthenticatedGateClient {
    /// Create a new authenticated client
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Result<Self, ClientError> {
        Self::new_with_timeout(base_url, api_key, None)
    }

    /// Create a new authenticated client with optional timeout
    fn new_with_timeout(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        timeout: Option<Duration>,
    ) -> Result<Self, ClientError> {
        let mut base_url = Url::parse(&base_url.into())
            .map_err(|e| ClientError::Configuration(format!("invalid base_url: {e}")))?;
        if base_url.path() != "/" && base_url.path().ends_with('/') {
            base_url = base_url
                .join(".")
                .map_err(|e| ClientError::Configuration(format!("invalid base_url path: {e}")))?;
        }
        let api_key = api_key.into();

        #[cfg(not(target_arch = "wasm32"))]
        let client = {
            let mut builder = ClientBuilder::new().user_agent("gate-client/0.1.0");
            if let Some(timeout) = timeout {
                builder = builder.timeout(timeout);
            }
            builder.build()?
        };

        #[cfg(target_arch = "wasm32")]
        let client = {
            let _ = timeout; // Timeouts not supported on WASM
            ClientBuilder::new()
                .user_agent("gate-client/0.1.0")
                .build()?
        };

        Ok(Self {
            client,
            base_url,
            api_key,
        })
    }

    /// Get the base URL
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Create a request builder with authentication
    pub fn request(&self, method: Method, path: &str) -> Result<RequestBuilder, ClientError> {
        let url = self
            .base_url
            .join(path.trim_start_matches('/'))
            .map_err(|e| ClientError::Configuration(format!("invalid path '{path}': {e}")))?;
        Ok(self
            .client
            .request(method, url)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.api_key)))
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

    /// Create a public client (useful for calling public endpoints)
    pub fn to_public(&self) -> PublicGateClient {
        PublicGateClient {
            client: self.client.clone(),
            base_url: self.base_url.clone(),
        }
    }
}

/// Type-safe builder that creates the appropriate client type
pub struct TypedClientBuilder {
    base_url: Option<String>,
    timeout: Option<Duration>,
}

impl TypedClientBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            base_url: None,
            timeout: None,
        }
    }

    /// Set the base URL
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Set the request timeout
    #[cfg(not(target_arch = "wasm32"))]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Build a public client
    pub fn build_public(self) -> Result<PublicGateClient, ClientError> {
        let base_url = self
            .base_url
            .ok_or_else(|| ClientError::Configuration("base_url is required".into()))?;

        PublicGateClient::new_with_timeout(base_url, self.timeout)
    }

    /// Build an authenticated client
    pub fn build_authenticated(
        self,
        api_key: impl Into<String>,
    ) -> Result<AuthenticatedGateClient, ClientError> {
        let base_url = self
            .base_url
            .ok_or_else(|| ClientError::Configuration("base_url is required".into()))?;

        AuthenticatedGateClient::new_with_timeout(base_url, api_key, self.timeout)
    }
}

impl Default for TypedClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}
