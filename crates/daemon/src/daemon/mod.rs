pub mod actor;
pub mod builder;
pub mod inner;
pub mod rpc;
pub mod server;

pub use builder::DaemonBuilder;
use gate_core::router::{ConnectorIndex, ConnectorRegistry};

use self::rpc::DaemonRequest;
use crate::Settings;
use crate::bootstrap::BootstrapTokenManager;
use crate::error::{DaemonError, Result};
use crate::permissions::LocalContext;
use crate::permissions::LocalIdentity;
use crate::services::WebAuthnService;
use crate::types::DaemonStatus;
use gate_core::access::SubjectIdentity;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

#[derive(Clone)]
pub struct Daemon {
    tx: mpsc::Sender<DaemonRequest>,
    identity: Option<LocalIdentity>,
    static_dir: Option<String>,
}

impl Daemon {
    pub fn new(tx: mpsc::Sender<DaemonRequest>, static_dir: Option<String>) -> Self {
        Self {
            tx,
            identity: None,
            static_dir,
        }
    }

    pub fn builder() -> DaemonBuilder {
        DaemonBuilder::new()
    }

    pub fn with_identity(mut self, identity: LocalIdentity) -> Self {
        self.identity = Some(identity);
        self
    }

    pub async fn with_http_identity(
        self,
        identity: &gate_http::services::HttpIdentity,
    ) -> Result<Self> {
        // Get state backend to do the conversion
        let state_backend = self.get_state_backend().await?;
        let local_ctx = LocalContext::from_http_identity(identity, state_backend.as_ref()).await;
        let local_identity =
            SubjectIdentity::new(identity.id.clone(), identity.source.clone(), local_ctx);
        Ok(self.with_identity(local_identity))
    }

    pub fn system_identity(&self) -> Self {
        let identity = SubjectIdentity::new(
            "system".to_string(),
            "system".to_string(), // source
            LocalContext {
                is_owner: true,
                node_id: "local".to_string(),
            },
        );
        self.clone().with_identity(identity)
    }

    pub async fn status(&self) -> Result<DaemonStatus> {
        let (reply, rx) = oneshot::channel();
        self.tx.send(DaemonRequest::GetStatus { reply }).await?;
        Ok(rx.await?)
    }

    pub async fn update_config(&self, config: Settings) -> Result<()> {
        let identity = self
            .identity
            .clone()
            .ok_or_else(|| DaemonError::InvalidState("No identity set".into()))?;

        let (reply, rx) = oneshot::channel();
        self.tx
            .send(DaemonRequest::UpdateConfig {
                identity,
                config: Box::new(config),
                reply,
            })
            .await?;
        rx.await?
    }

    pub async fn restart(&self) -> Result<()> {
        let identity = self
            .identity
            .clone()
            .ok_or_else(|| DaemonError::InvalidState("No identity set".into()))?;

        let (reply, rx) = oneshot::channel();
        self.tx
            .send(DaemonRequest::Restart { identity, reply })
            .await?;
        rx.await?
    }

    pub async fn shutdown(&self) -> Result<()> {
        let identity = self
            .identity
            .clone()
            .ok_or_else(|| DaemonError::InvalidState("No identity set".into()))?;

        let (reply, rx) = oneshot::channel();
        self.tx
            .send(DaemonRequest::Shutdown { identity, reply })
            .await?;
        rx.await?
    }

    pub async fn get_settings(&self) -> Result<Settings> {
        let (reply, rx) = oneshot::channel();
        self.tx.send(DaemonRequest::GetSettings { reply }).await?;
        Ok(rx.await?)
    }

    pub async fn get_bootstrap_manager(&self) -> Result<Arc<BootstrapTokenManager>> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(DaemonRequest::GetBootstrapManager { reply })
            .await?;
        Ok(rx.await?)
    }

    pub async fn get_webauthn_service(&self) -> Result<Option<Arc<WebAuthnService>>> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(DaemonRequest::GetWebAuthnService { reply })
            .await?;
        Ok(rx.await?)
    }

    pub async fn get_permission_manager(
        &self,
    ) -> Result<Arc<crate::permissions::LocalPermissionManager>> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(DaemonRequest::GetPermissionManager { reply })
            .await?;
        Ok(rx.await?)
    }

    pub async fn bootstrap_url(&self) -> Result<Option<String>> {
        let bootstrap_manager = self.get_bootstrap_manager().await?;
        let token = bootstrap_manager.get_token().await;
        let status = self.status().await?;
        Ok(token.map(|t| {
            let port = status.listen_address.split(':').nth(1).unwrap_or("31145");
            format!("http://localhost:{port}/bootstrap/{t}")
        }))
    }

    pub async fn server_address(&self) -> Result<String> {
        let status = self.status().await?;
        Ok(status.listen_address)
    }

    pub async fn user_count(&self) -> Result<usize> {
        let (reply, rx) = oneshot::channel();
        self.tx.send(DaemonRequest::GetUserCount { reply }).await?;
        Ok(rx.await?)
    }

    pub async fn get_auth_service(&self) -> Result<Arc<crate::services::AuthService>> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(DaemonRequest::GetAuthService { reply })
            .await?;
        Ok(rx.await?)
    }

    pub async fn get_state_backend(&self) -> Result<Arc<dyn gate_core::StateBackend>> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(DaemonRequest::GetStateBackend { reply })
            .await?;
        Ok(rx.await?)
    }

    pub async fn get_config(&self) -> Result<Settings> {
        let identity = self
            .identity
            .clone()
            .ok_or_else(|| DaemonError::InvalidState("No identity set".into()))?;

        let (reply, rx) = oneshot::channel();
        self.tx
            .send(DaemonRequest::GetConfig { identity, reply })
            .await?;
        rx.await?
    }

    /// Serve the daemon - uses ServerBuilder to reduce complexity
    pub async fn serve(self) -> Result<()> {
        // Get settings and create builder
        let settings = self.get_settings().await?;
        let builder = server::ServerBuilder::new(self.clone(), Arc::new(settings));

        // Step 1: Bind listener early to fail fast
        let listener = builder.bind_listener().await?;

        // Step 2: Get core services
        let state_backend = self.get_state_backend().await?;

        // Step 3: Initialize state and router (router is missing state)
        let state = builder.create_state().await?;
        let mut app_state = gate_http::AppState::new(state_backend.clone(), state);
        let router = builder.init_router();

        // Step 4: Setup connector registry and register all connectors
        let sink_registry = Arc::new(ConnectorRegistry::new());
        builder.register_sinks(&sink_registry).await?;

        // Step 5: Setup connector index
        let sink_index = Arc::new(ConnectorIndex::new());
        sink_index.refresh_from_registry(&sink_registry).await;

        // Step 6: Build core router with strategies and middleware
        let router_core = builder
            .build_router_core(state_backend, sink_registry, sink_index)
            .await;
        app_state = app_state.with_router(router_core);

        // Step 7: Build complete application with all middleware (still missing state)
        let app_missing_state = builder.build_app(router, app_state.clone()).await;

        // Step 8: Supply the actual state and serve the application
        let app = app_missing_state.with_state(app_state);
        axum::serve(listener, app).await.map_err(DaemonError::Io)?;

        Ok(())
    }
}
// axum 0.8: no ServiceExt needed
