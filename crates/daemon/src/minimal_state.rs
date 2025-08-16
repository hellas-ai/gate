//! Minimal state for HTTP routes
//!
//! This module provides the minimal state required for HTTP routes to function.
//! It contains just the auth service (for middleware) and the daemon handle (for business logic).

use crate::Daemon;
use gate_http::services::AuthService;
use std::sync::Arc;

/// Minimal state for HTTP routes
#[derive(Clone)]
pub struct MinimalState {
    /// Auth service for middleware authentication
    pub auth_service: Arc<AuthService>,
    /// Daemon handle for all business logic
    pub daemon: Daemon,
}

impl MinimalState {
    pub fn new(auth_service: Arc<AuthService>, daemon: Daemon) -> Self {
        Self {
            auth_service,
            daemon,
        }
    }
}

// Implement AsRef for auth middleware compatibility
impl AsRef<Arc<AuthService>> for MinimalState {
    fn as_ref(&self) -> &Arc<AuthService> {
        &self.auth_service
    }
}
