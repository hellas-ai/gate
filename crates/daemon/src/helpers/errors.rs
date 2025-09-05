//! Common error mapping utilities to reduce boilerplate

use gate_http::error::HttpError;
use std::fmt::Display;

/// Extension trait for Result types to provide common error mappings
pub trait ErrorMapExt<T> {
    /// Map any error to an InternalServerError with a formatted message
    fn map_internal_error(self) -> Result<T, HttpError>;
    
    /// Map any error to an InternalServerError with a custom context
    fn map_internal_error_with_context(self, context: &str) -> Result<T, HttpError>;
}

impl<T, E> ErrorMapExt<T> for Result<T, E>
where
    E: Display,
{
    fn map_internal_error(self) -> Result<T, HttpError> {
        self.map_err(|e| HttpError::InternalServerError(e.to_string()))
    }
    
    fn map_internal_error_with_context(self, context: &str) -> Result<T, HttpError> {
        self.map_err(|e| HttpError::InternalServerError(format!("{}: {}", context, e)))
    }
}

/// Helper function to create a NotFound error
pub fn not_found(entity: &str, id: &str) -> HttpError {
    HttpError::NotFound(format!("{} '{}' not found", entity, id))
}

/// Helper function to create a BadRequest error
pub fn bad_request(message: impl Into<String>) -> HttpError {
    HttpError::BadRequest(message.into())
}

/// Helper function to create a Forbidden error  
pub fn forbidden(message: impl Into<String>) -> HttpError {
    HttpError::Forbidden(message.into())
}