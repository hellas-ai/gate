//! API wrapper utilities for handling authentication errors

use crate::auth::{AuthAction, AuthContext};
use gate_http::client::error::ClientError;

/// Check if an error is an authentication failure that requires re-authentication
pub fn handle_api_error(error: &ClientError, auth: &AuthContext) {
    if error.is_auth_expired() {
        // Dispatch action to show re-auth modal
        auth.dispatch(AuthAction::ShowReauthModal);
    }
}

/// Wrapper for API calls that handles auth errors
pub async fn with_auth_error_handling<T, F>(
    auth: &AuthContext,
    api_call: F,
) -> Result<T, ClientError>
where
    F: std::future::Future<Output = Result<T, ClientError>>,
{
    match api_call.await {
        Ok(result) => Ok(result),
        Err(error) => {
            handle_api_error(&error, auth);
            Err(error)
        }
    }
}
