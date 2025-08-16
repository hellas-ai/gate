//! Minimal state for HTTP routes
//!
//! This module provides the minimal state required for HTTP routes to function.
//! It contains just the auth service (for middleware) and the daemon handle (for business logic).

use crate::Daemon;
use crate::services::AuthService;
use async_trait::async_trait;
use axum::http::request::Parts;
use gate_http::error::HttpError;
use gate_http::middleware::AuthProvider;
use gate_http::services::HttpIdentity;
use std::sync::Arc;

/// Minimal state for HTTP routes
#[derive(Clone)]
pub struct MinimalState {
    /// Auth service for middleware authentication
    pub auth_service: Arc<AuthService>,
    /// Daemon handle for all business logic
    pub daemon: Daemon,
}

impl MinimalState {
    pub fn new(auth_service: Arc<AuthService>, daemon: Daemon) -> Self {
        Self {
            auth_service,
            daemon,
        }
    }
}

// Implement AuthProvider directly for MinimalState
#[async_trait]
impl AuthProvider for MinimalState {
    async fn authenticate(&self, parts: &Parts) -> Result<HttpIdentity, HttpError> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                HttpError::AuthenticationFailed("Missing authorization header".to_string())
            })?;

        self.auth_service.authenticate_from_header(auth_header)
    }

    fn should_skip_auth(&self, path: &str) -> bool {
        path.starts_with("/auth/webauthn/")
            || path.starts_with("/auth/bootstrap/")
            || path == "/health"
            || path.starts_with("/swagger-ui")
            || path == "/"
            || path.ends_with(".js")
            || path.ends_with(".wasm")
            || path.ends_with(".html")
            || path.ends_with(".css")
    }
}
