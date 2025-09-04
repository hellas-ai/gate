use crate::Settings;
use crate::bootstrap::BootstrapTokenManager;
use crate::error::Result;
use crate::permissions::{LocalIdentity, LocalPermissionManager};
use crate::services::{AuthService, WebAuthnService};
use crate::types::DaemonStatus;
use gate_core::StateBackend;
use std::sync::Arc;
use tokio::sync::oneshot;

pub enum DaemonRequest {
    GetStatus {
        reply: oneshot::Sender<DaemonStatus>,
    },
    UpdateConfig {
        identity: LocalIdentity,
        config: Box<Settings>,
        reply: oneshot::Sender<Result<()>>,
    },
    Restart {
        identity: LocalIdentity,
        reply: oneshot::Sender<Result<()>>,
    },
    Shutdown {
        identity: LocalIdentity,
        reply: oneshot::Sender<Result<()>>,
    },
    GetSettings {
        reply: oneshot::Sender<Settings>,
    },
    GetBootstrapManager {
        reply: oneshot::Sender<Arc<BootstrapTokenManager>>,
    },
    GetWebAuthnService {
        reply: oneshot::Sender<Option<Arc<WebAuthnService>>>,
    },
    GetPermissionManager {
        reply: oneshot::Sender<Arc<LocalPermissionManager>>,
    },
    GetAuthService {
        reply: oneshot::Sender<Arc<AuthService>>,
    },
    GetStateBackend {
        reply: oneshot::Sender<Arc<dyn StateBackend>>,
    },
    GetUserCount {
        reply: oneshot::Sender<usize>,
    },
    GetConfig {
        identity: LocalIdentity,
        reply: oneshot::Sender<Result<Settings>>,
    },
}
