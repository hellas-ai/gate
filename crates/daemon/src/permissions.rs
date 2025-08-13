use async_trait::async_trait;
use gate_core::StateBackend;
use gate_core::access::{
    Action, IdentityContext, ObjectIdentity, PermissionDenied, PermissionManager, PermissionResult,
    Permissions, SubjectIdentity,
};
use gate_http::services::identity::HttpIdentity;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Local context for self-hosted deployments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalContext {
    pub is_owner: bool,
    pub node_id: String,
}

impl IdentityContext for LocalContext {
    fn to_attributes(&self) -> HashMap<String, String> {
        let mut attrs = HashMap::new();
        attrs.insert("node_id".to_string(), self.node_id.clone());
        attrs.insert("is_owner".to_string(), self.is_owner.to_string());
        attrs
    }

    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "node_id" => Some(&self.node_id),
            "is_owner" => Some(if self.is_owner { "true" } else { "false" }),
            _ => None,
        }
    }
}

impl LocalContext {
    /// Convert from HTTP identity to local context
    pub async fn from_http_identity(identity: &HttpIdentity, backend: &dyn StateBackend) -> Self {
        // Check if this is the first user (owner)
        let is_owner = backend
            .has_permission(
                &identity.id,
                &Action::GrantPermission,
                &ObjectIdentity::wildcard(),
            )
            .await
            .unwrap_or(false);

        Self {
            is_owner,
            node_id: identity
                .context
                .get("node_id")
                .unwrap_or("local")
                .to_string(),
        }
    }
}

pub type LocalIdentity = SubjectIdentity<LocalContext>;

pub struct LocalPermissionManager {
    backend: Arc<dyn StateBackend>,
}

impl LocalPermissionManager {
    pub fn new(backend: Arc<dyn StateBackend>) -> Self {
        Self { backend }
    }

    /// Initialize the first user as owner with all permissions
    pub async fn initialize_owner(&self, owner_id: &str) -> Result<(), PermissionDenied> {
        self.store_permission_unchecked(
            owner_id,
            Action::GrantPermission,
            &ObjectIdentity::wildcard(),
        )
        .await?;
        self.store_permission_unchecked(owner_id, Action::Manage, &ObjectIdentity::wildcard())
            .await?;
        Ok(())
    }
}

#[async_trait]
impl Permissions<LocalContext> for LocalPermissionManager {
    async fn check(
        &self,
        subject: &SubjectIdentity<LocalContext>,
        action: Action,
        object: &ObjectIdentity,
    ) -> PermissionResult {
        if subject.context.is_owner {
            return Ok(());
        }

        // Check database for specific permission
        match self
            .backend
            .has_permission(&subject.id, &action, object)
            .await
        {
            Ok(true) => Ok(()),
            Ok(false) => Err(PermissionDenied::NotAuthorized),
            Err(e) => Err(PermissionDenied::Custom(format!("Database error: {e}"))),
        }
    }
}

#[async_trait]
impl PermissionManager<LocalContext> for LocalPermissionManager {
    async fn grant(
        &self,
        granter: &SubjectIdentity<LocalContext>,
        grantee: &SubjectIdentity<LocalContext>,
        action: Action,
        object: &ObjectIdentity,
    ) -> PermissionResult {
        self.check(granter, Action::GrantPermission, object).await?;
        self.store_permission_unchecked(&grantee.id, action, object)
            .await
    }

    async fn revoke(
        &self,
        revoker: &SubjectIdentity<LocalContext>,
        subject: &SubjectIdentity<LocalContext>,
        action: Action,
        object: &ObjectIdentity,
    ) -> PermissionResult {
        self.check(revoker, Action::RevokePermission, object)
            .await?;

        match self
            .backend
            .remove_permission(&subject.id, &action, object)
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => Err(PermissionDenied::Custom(format!("Failed to revoke: {e}"))),
        }
    }

    async fn store_permission_unchecked(
        &self,
        subject_id: &str,
        action: Action,
        object: &ObjectIdentity,
    ) -> Result<(), PermissionDenied> {
        match self
            .backend
            .grant_permission(subject_id, &action, object)
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => Err(PermissionDenied::Custom(format!("Failed to store: {e}"))),
        }
    }
}
