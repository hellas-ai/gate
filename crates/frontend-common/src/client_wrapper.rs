//! Wrapped client that handles auth errors automatically

use gate_http::client::{error::ClientError, AuthenticatedGateClient};
use std::ops::Deref;

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

    /// Get a reference to the inner client
    pub fn inner(&self) -> &AuthenticatedGateClient {
        &self.inner
    }
}

// Allow the wrapper to be used like the inner client for method calls
impl Deref for WrappedAuthClient {
    type Target = AuthenticatedGateClient;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
