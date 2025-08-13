//! User-friendly error message mappings

/// Convert technical errors to user-friendly messages
pub fn get_user_friendly_error(error: &str) -> String {
    // Just return the error as-is for now
    // Server should provide user-friendly messages
    error.to_string()
}
