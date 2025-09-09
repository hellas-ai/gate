//! Service access wrapper to simplify daemon service retrieval patterns

use crate::daemon::Daemon;
use gate_core::state::StateBackend;
use gate_http::error::HttpError;
use std::sync::Arc;

/// Wrapper for commonly accessed daemon services
pub struct ServiceAccessor<'a> {
    daemon: &'a Daemon,
}

impl<'a> ServiceAccessor<'a> {
    /// Create a new service accessor
    pub fn new(daemon: &'a Daemon) -> Self {
        Self { daemon }
    }

    /// Get the permission manager with standard error handling
    pub async fn permission_manager(
        &self,
    ) -> Result<Arc<crate::permissions::LocalPermissionManager>, HttpError> {
        self.daemon.get_permission_manager().await.map_err(|e| {
            HttpError::InternalServerError(format!("Failed to get permission manager: {}", e))
        })
    }

    /// Get the state backend with standard error handling
    pub async fn state_backend(&self) -> Result<Arc<dyn StateBackend>, HttpError> {
        self.daemon.get_state_backend().await.map_err(|e| {
            HttpError::InternalServerError(format!("Failed to get state backend: {}", e))
        })
    }

    /// Get both permission manager and state backend (common pattern)
    pub async fn core_services(
        &self,
    ) -> Result<
        (
            Arc<crate::permissions::LocalPermissionManager>,
            Arc<dyn StateBackend>,
        ),
        HttpError,
    > {
        let permission_manager = self.permission_manager().await?;
        let state_backend = self.state_backend().await?;
        Ok((permission_manager, state_backend))
    }
}
