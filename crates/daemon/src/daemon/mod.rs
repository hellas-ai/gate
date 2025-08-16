pub mod actor;
pub mod builder;
pub mod inner;
pub mod rpc;

pub use builder::DaemonBuilder;
use utoipa_axum::router::OpenApiRouter;

use self::rpc::DaemonRequest;
use crate::Settings;
use crate::error::{DaemonError, Result};
use crate::permissions::LocalContext;
use crate::permissions::LocalIdentity;
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

    pub async fn get_bootstrap_manager(
        &self,
    ) -> Result<Arc<crate::bootstrap::BootstrapTokenManager>> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(DaemonRequest::GetBootstrapManager { reply })
            .await?;
        Ok(rx.await?)
    }

    pub async fn get_webauthn_service(
        &self,
    ) -> Result<Option<Arc<crate::services::WebAuthnService>>> {
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

    pub async fn get_upstream_registry(&self) -> Result<Arc<gate_http::UpstreamRegistry>> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(DaemonRequest::GetUpstreamRegistry { reply })
            .await?;
        Ok(rx.await?)
    }

    pub async fn get_inference_backend(
        &self,
    ) -> Result<Option<Arc<dyn gate_core::InferenceBackend>>> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(DaemonRequest::GetInferenceBackend { reply })
            .await?;
        Ok(rx.await?)
    }

    pub async fn get_config(&self) -> Result<serde_json::Value> {
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

    pub async fn serve(self) -> Result<()> {
        let settings = self.get_settings().await?;
        let addr = format!("{}:{}", settings.server.host, settings.server.port);
        let auth_service = self.get_auth_service().await?;
        let state_backend = self.get_state_backend().await?;
        let upstream_registry = self.get_upstream_registry().await?;
        let inference_backend = self.get_inference_backend().await?;

        // Build router with MinimalState containing daemon handle
        let router = OpenApiRouter::new();
        let router = crate::routes::auth::add_routes(router);
        let router = crate::routes::config::add_routes(router);
        let router = crate::routes::admin::add_routes(router);

        // Add inference-related routes from gate_http
        let router = gate_http::routes::models::add_routes(router);
        let router = gate_http::routes::inference::add_routes(router);
        let router = gate_http::routes::observability::add_routes(router);

        let minimal_state = crate::MinimalState::new(auth_service.clone(), self.clone());

        // Wrap MinimalState in AppState for middleware compatibility
        let mut app_state = gate_http::AppState::new(state_backend, minimal_state)
            .with_upstream_registry(upstream_registry);

        // Add inference backend if available
        if let Some(backend) = inference_backend {
            app_state = app_state.with_inference_backend(backend);
        }

        // Build the full axum app with middleware
        let mut app = router
            .split_for_parts()
            .0
            .with_state(app_state.clone())
            .route_layer(axum::middleware::from_fn_with_state(
                app_state.clone(),
                gate_http::middleware::auth::auth_middleware::<crate::MinimalState>,
            ))
            .layer(
                tower_http::cors::CorsLayer::new()
                    .allow_origin(tower_http::cors::Any)
                    .allow_methods(tower_http::cors::Any)
                    .allow_headers(vec![
                        axum::http::header::CONTENT_TYPE,
                        axum::http::header::AUTHORIZATION,
                        axum::http::HeaderName::from_static("x-correlation-id"),
                        axum::http::HeaderName::from_static("x-api-key"),
                        axum::http::HeaderName::from_static("traceparent"),
                        axum::http::HeaderName::from_static("tracestate"),
                    ])
                    .expose_headers(vec![
                        axum::http::HeaderName::from_static("x-correlation-id"),
                        axum::http::HeaderName::from_static("traceparent"),
                        axum::http::HeaderName::from_static("tracestate"),
                    ]),
            )
            .layer(axum::middleware::from_fn(
                gate_http::middleware::correlation_id_middleware,
            ));

        // Add static file serving if configured
        if let Some(static_dir) = &self.static_dir {
            if std::path::Path::new(static_dir).exists() {
                info!("Serving static files from: {}", static_dir);

                // Create static file service with SPA fallback
                // This serves files normally, but falls back to index.html for client-side routing
                let index_path = format!("{static_dir}/index.html");
                let serve_dir = tower_http::services::ServeDir::new(static_dir)
                    .fallback(tower_http::services::ServeFile::new(index_path));

                // Add catch-all route for static files and SPA
                app = app.fallback_service(serve_dir);
            } else {
                warn!("Static directory not found: {}", static_dir);
            }
        }

        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(DaemonError::Io)?;
        tracing::info!("Server listening on http://{}", addr);

        axum::serve(listener, app).await.map_err(DaemonError::Io)?;

        Ok(())
    }
}
