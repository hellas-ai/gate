//! State for HTTP routes
//!
//! This module provides the state required for HTTP routes to function.
//! It contains the auth service (for middleware) and the daemon handle (for business logic).

use crate::Daemon;
use crate::config::ProviderPassthroughConfig;
use crate::services::AuthService;
use async_trait::async_trait;
use axum::extract::connect_info::ConnectInfo;
use axum::http::HeaderName;
use axum::http::request::Parts;
use gate_core::router::signals::{anthropic_key_from, openai_bearer_from};
use gate_http::error::HttpError;
use gate_http::middleware::AuthProvider;
use gate_http::services::{HttpContext, HttpIdentity};
use std::net::SocketAddr;
use std::sync::Arc;

/// State for HTTP routes
#[derive(Clone)]
pub struct State {
    /// Auth service for middleware authentication
    pub auth_service: Arc<AuthService>,
    /// Daemon handle for all business logic
    pub daemon: Daemon,
    /// Whether to allow localhost peer to bypass auth
    pub allow_local_bypass: bool,
    /// Provider passthrough configuration
    pub provider_passthrough: ProviderPassthroughConfig,
}

impl State {
    pub fn new(
        auth_service: Arc<AuthService>,
        daemon: Daemon,
        allow_local_bypass: bool,
        provider_passthrough: ProviderPassthroughConfig,
    ) -> Self {
        Self {
            auth_service,
            daemon,
            allow_local_bypass,
            provider_passthrough,
        }
    }
}

// Implement AuthProvider directly for State
#[async_trait]
impl AuthProvider for State {
    async fn authenticate(&self, parts: &Parts) -> Result<HttpIdentity, HttpError> {
        // Helper: detect Anthropic API key from headers
        fn detect_anthropic_key(parts: &Parts) -> Option<String> {
            anthropic_key_from(&parts.headers).map(|s| s.to_string())
        }

        // Helper: detect OpenAI API key from headers
        fn detect_openai_key(parts: &Parts) -> Option<String> {
            if let Some(tok) = openai_bearer_from(&parts.headers) {
                return Some(tok.to_string());
            }
            if let Some(val) = parts
                .headers
                .get(HeaderName::from_static("x-api-key"))
                .and_then(|v| v.to_str().ok())
                && !val.is_empty()
            {
                return Some(val.to_string());
            }
            None
        }

        let path = parts.uri.path();
        // Provider-token auth bypass for Anthropic/OpenAI, gated by config
        let passthrough_allowed_path = self
            .provider_passthrough
            .allowed_paths
            .iter()
            .any(|p| p == path);
        let is_loopback = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip().is_loopback())
            .unwrap_or(false);
        if self.provider_passthrough.enabled
            && passthrough_allowed_path
            && (!self.provider_passthrough.loopback_only || is_loopback)
        {
            if let Some(_anthropic_key) = detect_anthropic_key(parts) {
                let identity = HttpIdentity::new(
                    "provider:anthropic".to_string(),
                    "anthropic-key".to_string(),
                    HttpContext::new()
                        .with_attribute("auth_method", "provider-key")
                        .with_attribute("provider", "anthropic")
                        .with_attribute("node_id", "local"),
                );
                return Ok(identity);
            }
            if let Some(_openai_key) = detect_openai_key(parts) {
                let identity = HttpIdentity::new(
                    "provider:openai".to_string(),
                    "openai-key".to_string(),
                    HttpContext::new()
                        .with_attribute("auth_method", "provider-key")
                        .with_attribute("provider", "openai")
                        .with_attribute("node_id", "local"),
                );
                return Ok(identity);
            }
        }

        if let Some(auth_header) = parts
            .headers
            .get("Authorization")
            .and_then(|value| value.to_str().ok())
        {
            // Default JWT auth
            return self.auth_service.authenticate_from_header(auth_header);
        }

        // If auth header is missing, allow localhost bypass when enabled and peer is loopback
        if self.allow_local_bypass {
            if let Some(connect_info) = parts.extensions.get::<ConnectInfo<SocketAddr>>() {
                let ip = connect_info.0.ip();
                if ip.is_loopback() {
                    let identity = HttpIdentity::new(
                        "local".to_string(),
                        "loopback".to_string(),
                        HttpContext::new()
                            .with_attribute("auth_method", "loopback")
                            .with_attribute("node_id", "local")
                            .with_attribute("is_owner", "true"),
                    );
                    info!("Granted localhost bypass for peer {}", ip);
                    return Ok(identity);
                } else {
                    debug!("Localhost bypass not applied: peer {} not loopback", ip);
                }
            } else {
                debug!("Localhost bypass not applied: missing ConnectInfo");
            }
        }

        Err(HttpError::AuthenticationFailed(
            "Missing authorization header".to_string(),
        ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::rpc::DaemonRequest;
    use axum::http::Request;
    use gate_core::access::IdentityContext;
    use gate_http::services::JwtService;
    use gate_http::services::jwt::JwtConfig as JwtSvcConfig;
    use gate_sqlx::SqliteStateBackend;
    use tokio::sync::mpsc;

    async fn make_minimal_state(allow_local_bypass: bool) -> State {
        // Build a lightweight AuthService stack (will not be exercised in these tests)
        let jwt_service = Arc::new(JwtService::new(JwtSvcConfig::new(
            "test-secret".to_string(),
            24,
            "test-issuer".to_string(),
        )));

        // In-memory state backend and webauthn backend
        let state_backend = Arc::new(SqliteStateBackend::new(":memory:").await.unwrap());
        let webauthn_backend = Arc::new(gate_sqlx::SqliteWebAuthnBackend::new(
            state_backend.pool().clone(),
        ));

        let auth_service = Arc::new(AuthService::new(
            jwt_service,
            state_backend,
            webauthn_backend,
        ));

        // Dummy daemon handle
        let (tx, _rx) = mpsc::channel::<DaemonRequest>(1);
        let daemon = crate::daemon::Daemon::new(tx, None);

        State::new(
            auth_service,
            daemon,
            allow_local_bypass,
            crate::config::ProviderPassthroughConfig::default(),
        )
    }

    #[tokio::test]
    async fn test_loopback_bypass_allows_without_auth() {
        let state = make_minimal_state(true).await;

        let req: Request<()> = Request::builder().uri("/api/config").body(()).unwrap();
        let (mut parts, _body) = req.into_parts();
        parts
            .extensions
            .insert(ConnectInfo::<SocketAddr>(SocketAddr::from((
                [127, 0, 0, 1],
                1234,
            ))));

        let ident = state
            .authenticate(&parts)
            .await
            .expect("expected bypass identity");
        assert_eq!(ident.source, "loopback");
        assert_eq!(ident.context.get("is_owner"), Some("true"));
    }

    #[tokio::test]
    async fn test_non_loopback_rejects_without_auth() {
        let state = make_minimal_state(true).await;

        let req: Request<()> = Request::builder().uri("/").body(()).unwrap();
        let (mut parts, _body) = req.into_parts();
        parts
            .extensions
            .insert(ConnectInfo::<SocketAddr>(SocketAddr::from((
                [192, 168, 1, 10],
                5555,
            ))));

        let res = state.authenticate(&parts).await;
        assert!(matches!(res, Err(HttpError::AuthenticationFailed(_))));
    }

    #[tokio::test]
    async fn test_loopback_rejects_when_toggle_disabled() {
        let state = make_minimal_state(false).await;

        let req: Request<()> = Request::builder().uri("/").body(()).unwrap();
        let (mut parts, _body) = req.into_parts();
        parts
            .extensions
            .insert(ConnectInfo::<SocketAddr>(SocketAddr::from((
                [127, 0, 0, 1],
                9999,
            ))));

        let res = state.authenticate(&parts).await;
        assert!(matches!(res, Err(HttpError::AuthenticationFailed(_))));
    }
}
