//! User-friendly error message mappings

use crate::client::ClientError;

/// Convert technical errors to user-friendly messages
pub fn get_user_friendly_error(error: &ClientError) -> String {
    // Convert specific error types to user-friendly messages
    match error {
        ClientError::AuthenticationFailed(msg) if msg.contains("InvalidSignature") => {
            "Your session has expired. Please re-authenticate.".to_string()
        }
        ClientError::AuthenticationFailed(msg) => {
            format!("Authentication failed: {msg}")
        }
        ClientError::NotFound(_) => "The requested resource was not found.".to_string(),
        ClientError::Configuration(msg) => {
            format!("Configuration error: {msg}")
        }
        _ => {
            // For other errors, just use the default display
            error.to_string()
        }
    }
}
