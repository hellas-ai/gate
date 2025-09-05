//! HTTP error types and implementations

#[cfg(feature = "server")]
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// HTTP-specific errors
#[derive(Error, Debug)]
pub enum HttpError {
    /// Authentication failed
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Authorization failed
    #[error("Authorization failed: {0}")]
    AuthorizationFailed(String),

    /// Resource not found
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// Bad request
    #[error("Bad request: {0}")]
    BadRequest(String),

    /// Internal server error
    #[error("Internal server error: {0}")]
    InternalServerError(String),

    /// Service unavailable
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    /// Conflict
    #[error("Conflict: {0}")]
    Conflict(String),

    /// Unprocessable entity
    #[error("Unprocessable entity: {0}")]
    UnprocessableEntity(String),

    /// Not implemented
    #[error("Not implemented: {0}")]
    NotImplemented(String),

    /// Core error
    #[error(transparent)]
    Core(#[from] gate_core::Error),
}

/// Error response body
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[cfg(feature = "server")]
impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let (status, error_type) = match &self {
            HttpError::AuthenticationFailed(_) => {
                (StatusCode::UNAUTHORIZED, "authentication_failed")
            }
            HttpError::AuthorizationFailed(_) => (StatusCode::FORBIDDEN, "authorization_failed"),
            HttpError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            HttpError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            HttpError::InternalServerError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_server_error")
            }
            HttpError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable")
            }
            HttpError::RateLimitExceeded => (StatusCode::TOO_MANY_REQUESTS, "rate_limit_exceeded"),
            HttpError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            HttpError::UnprocessableEntity(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "unprocessable_entity")
            }
            HttpError::NotImplemented(_) => (StatusCode::from_u16(501).unwrap(), "not_implemented"),
            HttpError::Core(core_err) => {
                use gate_core::Error;
                match core_err {
                    Error::Rejected(status, _) => (*status, "upstream_error"),
                    Error::Unauthorized => (StatusCode::FORBIDDEN, "unauthorized"),
                    Error::ApiKeyNotFound | Error::InvalidApiKey => {
                        (StatusCode::UNAUTHORIZED, "invalid_api_key")
                    }
                    Error::ModelNotFound(_) | Error::ProviderNotFound(_) => {
                        (StatusCode::NOT_FOUND, "not_found")
                    }
                    Error::QuotaExceeded(_) => (StatusCode::TOO_MANY_REQUESTS, "quota_exceeded"),
                    Error::InvalidRequest(_) => (StatusCode::BAD_REQUEST, "invalid_request"),
                    Error::ServiceUnavailable(_) => {
                        (StatusCode::SERVICE_UNAVAILABLE, "service_unavailable")
                    }
                    _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal_server_error"),
                }
            }
        };

        let body = ErrorResponse {
            error: error_type.to_string(),
            message: self.to_string(),
            details: None,
        };

        (status, Json(body)).into_response()
    }
}

/// Result type alias using HttpError
pub type Result<T> = std::result::Result<T, HttpError>;
