use crate::bootstrap::BootstrapTokenManager;
use crate::error::Result;
use crate::permissions::{LocalIdentity, LocalPermissionManager};
use crate::services::{AuthService, TlsForwardService, WebAuthnService};
use crate::types::{DaemonStatus, TlsForwardStatus};
use crate::{Settings, state_dir::StateDir};
use gate_core::StateBackend;
use gate_core::access::{
    Action, ObjectId, ObjectIdentity, ObjectKind, Permissions, TargetNamespace,
};
use gate_http::services::JwtService;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct DaemonInner {
    settings: Arc<RwLock<Settings>>,
    state_backend: Arc<dyn StateBackend>,
    permission_manager: Arc<LocalPermissionManager>,
    auth_service: Arc<AuthService>,
    jwt_service: Arc<JwtService>,
    bootstrap_manager: Arc<BootstrapTokenManager>,
    webauthn_service: Option<Arc<WebAuthnService>>,
    tlsforward_service: Option<Arc<TlsForwardService>>,
    user_count: usize,
}

impl DaemonInner {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        settings: Settings,
        state_backend: Arc<dyn StateBackend>,
        auth_service: Arc<AuthService>,
        jwt_service: Arc<JwtService>,
        bootstrap_manager: Arc<BootstrapTokenManager>,
        webauthn_service: Option<Arc<WebAuthnService>>,
        tlsforward_service: Option<Arc<TlsForwardService>>,
        user_count: usize,
    ) -> Self {
        let permission_manager = Arc::new(LocalPermissionManager::new(state_backend.clone()));

        Self {
            settings: Arc::new(RwLock::new(settings)),
            state_backend,
            permission_manager,
            auth_service,
            jwt_service,
            bootstrap_manager,
            webauthn_service,
            tlsforward_service,
            user_count,
        }
    }

    pub async fn status(&self) -> DaemonStatus {
        let settings = self.settings.read().await;
        DaemonStatus {
            running: true,
            listen_address: format!("{}:{}", settings.server.host, settings.server.port),
            provider_count: settings.providers.len(),
            user_count: self.user_count,
            tlsforward_enabled: self.tlsforward_service.is_some(),
            tlsforward_status: self.get_tlsforward_status().await,
            needs_bootstrap: self.user_count == 0,
        }
    }

    pub async fn update_config(
        &mut self,
        identity: &LocalIdentity,
        config: Settings,
    ) -> Result<()> {
        let config_object = ObjectIdentity {
            namespace: TargetNamespace::System,
            kind: ObjectKind::Config,
            id: ObjectId::new("*"),
        };

        self.permission_manager
            .check(identity, Action::Write, &config_object)
            .await?;

        *self.settings.write().await = config;

        // Persist to default config path
        if let Ok(state_dir) = StateDir::new().await {
            let path = state_dir.config_path();
            if let Err(e) = self.settings.read().await.save_to_file(&path).await {
                tracing::warn!("Failed to save settings to {}: {}", path.display(), e);
            } else {
                tracing::info!("Saved settings to {}", path.display());
            }
        } else {
            tracing::warn!("Failed to resolve state dir; settings not persisted to disk");
        }

        self.reload_services().await?;
        Ok(())
    }

    pub async fn restart(&mut self, identity: &LocalIdentity) -> Result<()> {
        let daemon_object = ObjectIdentity {
            namespace: TargetNamespace::System,
            kind: ObjectKind::System,
            id: ObjectId::new("daemon"),
        };

        self.permission_manager
            .check(identity, Action::Manage, &daemon_object)
            .await?;

        self.shutdown_internal().await?;
        self.start_internal().await?;
        Ok(())
    }

    pub async fn shutdown(&mut self, identity: &LocalIdentity) -> Result<()> {
        let daemon_object = ObjectIdentity {
            namespace: TargetNamespace::System,
            kind: ObjectKind::System,
            id: ObjectId::new("daemon"),
        };

        self.permission_manager
            .check(identity, Action::Manage, &daemon_object)
            .await?;

        self.shutdown_internal().await
    }

    pub fn get_user_count(&self) -> usize {
        self.user_count
    }

    pub fn get_auth_service(&self) -> Arc<AuthService> {
        self.auth_service.clone()
    }

    pub fn get_state_backend(&self) -> Arc<dyn StateBackend> {
        self.state_backend.clone()
    }

    pub fn get_bootstrap_manager(&self) -> Arc<BootstrapTokenManager> {
        self.bootstrap_manager.clone()
    }

    pub fn get_webauthn_service(&self) -> Option<Arc<WebAuthnService>> {
        self.webauthn_service.clone()
    }

    pub fn get_permission_manager(&self) -> Arc<LocalPermissionManager> {
        self.permission_manager.clone()
    }

    pub fn get_jwt_service(&self) -> Arc<JwtService> {
        self.jwt_service.clone()
    }

    pub async fn get_settings(&self) -> Settings {
        self.settings.read().await.clone()
    }

    pub async fn get_config(&self, identity: &LocalIdentity) -> Result<Settings> {
        // Check permission to read configuration
        let config_object = ObjectIdentity {
            namespace: TargetNamespace::System,
            kind: ObjectKind::Config,
            id: ObjectId::new("*"),
        };

        self.permission_manager
            .check(identity, Action::Read, &config_object)
            .await?;

        // Get the current configuration
        let current_config = self.settings.read().await.clone();
        Ok(current_config)
    }

    async fn get_tlsforward_status(&self) -> TlsForwardStatus {
        if let Some(service) = &self.tlsforward_service {
            let state = service.subscribe().borrow().clone();
            match state {
                crate::services::TlsForwardState::Disconnected => TlsForwardStatus::Disconnected,
                crate::services::TlsForwardState::Connecting => TlsForwardStatus::Connecting,
                crate::services::TlsForwardState::Connected {
                    assigned_domain, ..
                } => TlsForwardStatus::Connected {
                    domain: assigned_domain,
                },
                crate::services::TlsForwardState::Error(error) => TlsForwardStatus::Error(error),
            }
        } else {
            TlsForwardStatus::Disabled
        }
    }

    async fn reload_services(&mut self) -> Result<()> {
        // TODO: Implement service reloading logic
        Ok(())
    }

    async fn shutdown_internal(&mut self) -> Result<()> {
        if let Some(service) = &self.tlsforward_service {
            service.shutdown().await;
        }
        Ok(())
    }

    async fn start_internal(&mut self) -> Result<()> {
        // TODO: Implement service startup logic
        Ok(())
    }
}
