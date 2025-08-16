use crate::error::HttpError;
use crate::services::identity::HttpIdentity;
use async_trait::async_trait;
use axum::{extract::Request, http::request::Parts, middleware::Next, response::Response};

/// Trait for authentication providers
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// Authenticate a request and return identity if successful
    async fn authenticate(&self, parts: &Parts) -> Result<HttpIdentity, HttpError>;

    /// Check if authentication should be skipped for a given path
    fn should_skip_auth(&self, path: &str) -> bool {
        path == "/health" || path.starts_with("/swagger-ui") || path == "/"
    }
}

/// Middleware function for authentication
pub async fn auth_middleware<T>(
    axum::extract::State(app_state): axum::extract::State<crate::AppState<T>>,
    req: Request,
    next: Next,
) -> Result<Response, HttpError>
where
    crate::AppState<T>: AuthProvider,
    T: Clone + Send + Sync + 'static,
{
    let path = req.uri().path();

    if app_state.should_skip_auth(path) {
        return Ok(next.run(req).await);
    }

    let (mut parts, body) = req.into_parts();

    match app_state.authenticate(&parts).await {
        Ok(identity) => {
            parts.extensions.insert(identity);
            let req = Request::from_parts(parts, body);
            Ok(next.run(req).await)
        }
        Err(e) => Err(e),
    }
}

/// Blanket implementation for AppState<T> where T implements AuthProvider
#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl<T> AuthProvider for crate::AppState<T>
where
    T: AuthProvider + Clone + Send + Sync,
{
    async fn authenticate(&self, parts: &Parts) -> Result<HttpIdentity, HttpError> {
        self.data.authenticate(parts).await
    }

    fn should_skip_auth(&self, path: &str) -> bool {
        self.data.should_skip_auth(path)
    }
}
