use super::identity::{IdentityContext, ObjectIdentity, SubjectIdentity};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

/// Actions that can be performed on objects
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    Read,
    Write,
    Delete,
    Execute,
    Manage,
    GrantPermission,
    RevokePermission,
    ViewPermissions,
    ViewQuota,
    UpdateQuota,
    ConsumeQuota,
}

/// Reasons for permission denial
#[derive(Debug, Clone, Error)]
pub enum PermissionDenied {
    #[error("Not authorized")]
    NotAuthorized,

    #[error("Permission expired")]
    Expired,

    #[error("Rate limited, retry after {retry_after:?}")]
    RateLimited { retry_after: Duration },

    #[error("Quota exceeded for {resource}")]
    QuotaExceeded { resource: String },

    #[error("Account suspended: {reason}")]
    Suspended { reason: String },

    #[error("Permission denied: {0}")]
    Custom(String),
}

pub type PermissionResult = Result<(), PermissionDenied>;

/// Unified permissions interface (check + grant/revoke/store)
#[async_trait]
pub trait Permissions<C: IdentityContext>: Send + Sync {
    /// Check if a subject may perform an action on an object
    async fn check(
        &self,
        subject: &SubjectIdentity<C>,
        action: Action,
        object: &ObjectIdentity,
    ) -> PermissionResult;

    /// Convenience wrapper around `check`
    async fn require(
        &self,
        subject: &SubjectIdentity<C>,
        action: Action,
        object: &ObjectIdentity,
    ) -> PermissionResult {
        self.check(subject, action, object).await
    }

    /// Grant a permission to a subject
    async fn grant(
        &self,
        granter: &SubjectIdentity<C>,
        grantee: &SubjectIdentity<C>,
        action: Action,
        object: &ObjectIdentity,
    ) -> PermissionResult;

    /// Revoke a permission from a subject
    async fn revoke(
        &self,
        revoker: &SubjectIdentity<C>,
        subject: &SubjectIdentity<C>,
        action: Action,
        object: &ObjectIdentity,
    ) -> PermissionResult;

    /// Store a permission without checking granter's permissions
    async fn store_permission_unchecked(
        &self,
        subject_id: &str,
        action: Action,
        object: &ObjectIdentity,
    ) -> Result<(), PermissionDenied>;
}
