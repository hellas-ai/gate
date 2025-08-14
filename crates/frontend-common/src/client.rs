//! Client configuration and initialization

use crate::client_wrapper::WrappedAuthClient;
pub use gate_http::client::error::ClientError;
use gate_http::client::{PublicGateClient, TypedClientBuilder};
use once_cell::sync::Lazy;
use std::sync::Mutex;
use web_sys::window;

/// Global client instances
static PUBLIC_CLIENT: Lazy<Mutex<Option<PublicGateClient>>> = Lazy::new(|| Mutex::new(None));
static AUTH_CLIENT: Lazy<Mutex<Option<WrappedAuthClient>>> = Lazy::new(|| Mutex::new(None));

/// Get the base URL for API calls
fn get_base_url() -> String {
    // Try to get from window location
    if let Some(window) = window() {
        if let Ok(location) = window.location().origin() {
            return location;
        }
    }

    // Default to relative URLs
    String::new()
}

/// Get the public client instance (for unauthenticated endpoints)
pub fn create_public_client() -> Result<PublicGateClient, ClientError> {
    let mut client_lock = PUBLIC_CLIENT
        .lock()
        .expect("Failed to acquire public client lock");

    if client_lock.is_none() {
        let client = TypedClientBuilder::new()
            .base_url(get_base_url())
            .build_public()?;
        *client_lock = Some(client.clone());
        Ok(client)
    } else {
        Ok(client_lock
            .as_ref()
            .expect("Public client should be initialized")
            .clone())
    }
}

/// Get the authenticated client instance (returns None if not authenticated)
pub fn create_authenticated_client() -> Result<Option<WrappedAuthClient>, ClientError> {
    let client_lock = AUTH_CLIENT
        .lock()
        .expect("Failed to acquire auth client lock");
    Ok(client_lock.clone())
}

/// Update the typed clients with an authentication token
pub fn set_auth_token(token: Option<&str>) -> Result<(), ClientError> {
    let mut auth_lock = AUTH_CLIENT
        .lock()
        .expect("Failed to acquire auth client lock");

    if let Some(token) = token {
        // Create authenticated client and wrap it
        let auth_client = TypedClientBuilder::new()
            .base_url(get_base_url())
            .build_authenticated(token)?;
        *auth_lock = Some(WrappedAuthClient::new(auth_client));
    } else {
        // Clear authenticated client
        *auth_lock = None;
    }

    Ok(())
}
