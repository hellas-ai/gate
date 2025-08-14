//! Client error types

use thiserror::Error;

/// Client error types
#[derive(Debug, Error)]
pub enum ClientError {
    /// Network or request error
    #[error("Request failed: {0}")]
    Request(#[from] reqwest::Error),

    /// Server returned an error status
    #[error("Server error {status}: {message}")]
    ServerError { status: u16, message: String },

    /// Authentication failed
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Resource not found
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// Bad request
    #[error("Bad request: {0}")]
    BadRequest(String),

    /// Forbidden
    #[error("Forbidden: {0}")]
    Forbidden(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    Configuration(String),
}

impl ClientError {
    /// Create error from HTTP status code
    pub fn from_status(status: reqwest::StatusCode, message: String) -> Self {
        match status.as_u16() {
            400 => Self::BadRequest(message),
            401 => Self::AuthenticationFailed(message),
            403 => Self::Forbidden(message),
            404 => Self::NotFound(message),
            _ => Self::ServerError {
                status: status.as_u16(),
                message,
            },
        }
    }

    /// Check if this is an authentication error that requires re-authentication
    pub fn is_auth_expired(&self) -> bool {
        match self {
            Self::AuthenticationFailed(_) => true, // All 401 errors should trigger re-auth
            _ => false,
        }
    }
}
