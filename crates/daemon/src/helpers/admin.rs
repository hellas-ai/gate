//! Admin route helper functions to reduce repetitive permission checking code

use crate::permissions::LocalContext;
use gate_core::access::{
    Action, ObjectIdentity, PermissionManager, SubjectIdentity,
};
use gate_core::state::StateBackend;
use gate_http::{error::HttpError, services::HttpIdentity};
use std::sync::Arc;

/// Helper struct to handle common admin permission checks
pub struct AdminPermissionHelper {
    pub permission_manager: Arc<dyn PermissionManager>,
    pub state_backend: Arc<dyn StateBackend>,
    pub identity: HttpIdentity,
    pub local_identity: SubjectIdentity,
}

impl AdminPermissionHelper {
    /// Create a new AdminPermissionHelper from app state and identity
    pub async fn new(
        daemon: &crate::daemon::GateDaemon,
        identity: HttpIdentity,
    ) -> Result<Self, HttpError> {
        let permission_manager = daemon
            .get_permission_manager()
            .await
            .map_err(|e| HttpError::InternalServerError(e.to_string()))?;
        
        let state_backend = daemon
            .get_state_backend()
            .await
            .map_err(|e| HttpError::InternalServerError(e.to_string()))?;
        
        let local_ctx = LocalContext::from_http_identity(&identity, state_backend.as_ref()).await;
        let local_identity = SubjectIdentity::new(
            identity.id.clone(),
            identity.source.clone(),
            local_ctx,
        );
        
        Ok(Self {
            permission_manager,
            state_backend,
            identity,
            local_identity,
        })
    }
    
    /// Check if the user has permission to perform an action on an object
    pub async fn check_permission(
        &self,
        action: Action,
        object: &ObjectIdentity,
    ) -> Result<(), HttpError> {
        self.permission_manager
            .check(&self.local_identity, action, object)
            .await
            .map_err(|e| {
                tracing::debug!(
                    "Permission denied for user {} on object {:?}: {}",
                    self.identity.id,
                    object,
                    e
                );
                HttpError::Forbidden(format!("Insufficient permissions: {}", e))
            })
    }
    
    /// Check if the user has admin permissions for a specific action
    pub async fn require_admin(
        &self,
        action: Action,
        object: &ObjectIdentity,
    ) -> Result<(), HttpError> {
        self.check_permission(action, object).await
    }
}