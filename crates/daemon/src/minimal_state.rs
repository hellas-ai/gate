//! Minimal state for HTTP routes
//!
//! This module provides the minimal state required for HTTP routes to function.
//! It contains just the auth service (for middleware) and the daemon handle (for business logic).

use crate::Daemon;
use crate::services::AuthService;
use async_trait::async_trait;
use axum::extract::connect_info::ConnectInfo;
use axum::http::request::Parts;
use gate_http::error::HttpError;
use gate_http::middleware::AuthProvider;
use gate_http::services::{HttpContext, HttpIdentity};
use std::net::SocketAddr;
use std::sync::Arc;

/// Minimal state for HTTP routes
#[derive(Clone)]
pub struct MinimalState {
    /// Auth service for middleware authentication
    pub auth_service: Arc<AuthService>,
    /// Daemon handle for all business logic
    pub daemon: Daemon,
    /// Whether to allow localhost peer to bypass auth
    pub allow_local_bypass: bool,
}

impl MinimalState {
    pub fn new(auth_service: Arc<AuthService>, daemon: Daemon, allow_local_bypass: bool) -> Self {
        Self {
            auth_service,
            daemon,
            allow_local_bypass,
        }
    }
}

// Implement AuthProvider directly for MinimalState
#[async_trait]
impl AuthProvider for MinimalState {
    async fn authenticate(&self, parts: &Parts) -> Result<HttpIdentity, HttpError> {
        if let Some(auth_header) = parts
            .headers
            .get("Authorization")
            .and_then(|value| value.to_str().ok())
        {
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

    async fn make_minimal_state(allow_local_bypass: bool) -> MinimalState {
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

        MinimalState::new(auth_service, daemon, allow_local_bypass)
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
