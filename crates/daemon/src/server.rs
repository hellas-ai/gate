//! Server setup and configuration module

use crate::ServerState;
use crate::bootstrap::BootstrapTokenManager;
use crate::config::Settings;
use crate::permissions::LocalPermissionManager;
use anyhow::Result;
use axum;
use gate_core::{StateBackend, WebAuthnBackend};
use gate_http::{
    AppState, UpstreamRegistry,
    forwarding::ForwardingConfig,
    middleware::WebAuthnConfig,
    model_detection,
    services::{AuthService, JwtConfig, JwtService, WebAuthnService},
};
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::{debug, info, warn};
use utoipa_axum::router::OpenApiRouter;
use utoipa_scalar::{Scalar, Servable as _};

/// Server configuration builder
pub struct ServerBuilder {
    settings: Settings,
    state_backend: Arc<dyn StateBackend>,
    webauthn_backend: Arc<dyn WebAuthnBackend>,
    settings_arc: Arc<Settings>,
    bootstrap_manager: Option<Arc<BootstrapTokenManager>>,
}

impl ServerBuilder {
    /// Create a new server builder
    pub fn new(
        settings: Settings,
        state_backend: Arc<dyn StateBackend>,
        webauthn_backend: Arc<dyn WebAuthnBackend>,
        settings_arc: Arc<Settings>,
    ) -> Self {
        Self {
            settings,
            state_backend,
            webauthn_backend,
            settings_arc,
            bootstrap_manager: None,
        }
    }

    /// Set the bootstrap manager
    pub fn with_bootstrap_manager(mut self, bootstrap_manager: Arc<BootstrapTokenManager>) -> Self {
        self.bootstrap_manager = Some(bootstrap_manager);
        self
    }

    /// Build the JWT service
    pub fn build_jwt_service(&self) -> Arc<JwtService> {
        let jwt_config = JwtConfig {
            secret: self.settings.auth.jwt.secret.clone(),
            expiration: chrono::Duration::hours(self.settings.auth.jwt.expiration_hours as i64),
            issuer: self.settings.auth.jwt.issuer.clone(),
        };
        Arc::new(JwtService::new(jwt_config))
    }

    /// Build the upstream registry
    pub async fn build_upstream_registry(&self) -> Result<Arc<UpstreamRegistry>> {
        let upstream_registry = Arc::new(UpstreamRegistry::new());

        if !self.settings.upstreams.is_empty() {
            info!(
                "Initializing {} upstream providers",
                self.settings.upstreams.len()
            );

            for upstream_config in &self.settings.upstreams {
                info!(
                    "Configuring upstream '{}' with provider: {}",
                    upstream_config.name, upstream_config.provider
                );

                let config = ForwardingConfig {
                    provider: upstream_config.provider.clone(),
                    base_url: upstream_config.base_url.clone(),
                    api_key: upstream_config.api_key.clone(),
                    timeout_seconds: upstream_config.timeout_seconds,
                };

                // Detect models for this upstream
                info!(
                    "Detecting models for upstream '{}'...",
                    upstream_config.name
                );
                let models = model_detection::detect_models(&config).await;

                if models.is_empty() {
                    info!(
                        "No models detected for upstream '{}', it may be offline or not support model listing",
                        upstream_config.name
                    );
                } else {
                    info!(
                        "Detected {} models for upstream '{}'",
                        models.len(),
                        upstream_config.name
                    );
                }

                upstream_registry
                    .register_upstream(upstream_config.name.clone(), config, models)
                    .await;
            }
        } else {
            debug!("No upstreams configured");
        }

        Ok(upstream_registry)
    }

    /// Build the inference service if configured
    async fn build_inference_service(&self) -> Option<Arc<dyn gate_core::InferenceBackend>> {
        if let Some(inference_config) = &self.settings.local_inference {
            debug!("Initializing local inference service");

            // Use the config as-is
            let config = inference_config.clone();

            match crate::services::LocalInferenceServiceBuilder::new(config).build() {
                Ok(inference_service) => {
                    let inference_backend: Arc<dyn gate_core::InferenceBackend> =
                        Arc::new(inference_service);

                    // List available models
                    if let Ok(models) = inference_backend.list_models().await {
                        info!("Loaded {} local models:", models.len());
                        for model in models {
                            info!("  - {} ({})", model.id, model.name);
                        }
                    }

                    Some(inference_backend)
                }
                Err(e) => {
                    warn!("Failed to initialize local inference service: {}", e);
                    info!("Continuing without local inference support");
                    None
                }
            }
        } else {
            None
        }
    }

    /// Build the app state
    pub async fn build_app_state(
        &self,
        jwt_service: Arc<JwtService>,
        upstream_registry: Arc<UpstreamRegistry>,
    ) -> Result<AppState<ServerState>> {
        // Build inference service if configured
        let inference_backend = self.build_inference_service().await;

        let state = if self.settings.auth.webauthn.enabled {
            // Use WebAuthn configuration from settings
            let webauthn_config = WebAuthnConfig {
                rp_id: self.settings.auth.webauthn.rp_id.clone(),
                rp_name: self.settings.auth.webauthn.rp_name.clone(),
                rp_origin: self.settings.auth.webauthn.rp_origin.clone(),
                allowed_origins: self.settings.auth.webauthn.allowed_origins.clone(),
                allow_tlsforward_origins: self.settings.auth.webauthn.allow_tlsforward_origins,
                allow_subdomains: self.settings.auth.webauthn.allow_subdomains,
                require_user_verification: self.settings.auth.webauthn.require_user_verification,
                session_timeout_seconds: self.settings.auth.webauthn.session_timeout_seconds,
            };

            // Use the WebAuthn backend provided to the builder
            let webauthn_backend = self.webauthn_backend.clone();

            // Log the WebAuthn configuration being used
            debug!(
                "Creating WebAuthn service with RP ID: '{}', RP Origin: '{}', Allow Relay: {}",
                webauthn_config.rp_id,
                webauthn_config.rp_origin,
                webauthn_config.allow_tlsforward_origins
            );

            // Create WebAuthn service
            let webauthn_service = Arc::new(
                WebAuthnService::new(webauthn_config.clone(), webauthn_backend.clone())
                    .map_err(|e| anyhow::anyhow!("Failed to create WebAuthn service: {}", e))?,
            );

            // Create Auth service
            let auth_service = Arc::new(AuthService::new(
                jwt_service.clone(),
                self.state_backend.clone(),
                webauthn_backend.clone(),
            ));

            // Use the bootstrap manager from builder (required)
            let bootstrap_manager = self
                .bootstrap_manager
                .clone()
                .expect("Bootstrap manager must be provided to ServerBuilder");

            // Create permission manager
            let permission_manager =
                Arc::new(LocalPermissionManager::new(self.state_backend.clone()));

            // Create server state with WebAuthn
            let server_state = ServerState {
                auth_service,
                webauthn_service: Some(webauthn_service),
                jwt_service,
                settings: self.settings_arc.clone(),
                bootstrap_manager,
                inference_service: inference_backend.clone(),
                permission_manager,
            };

            let mut app_state = AppState::new(self.state_backend.clone(), server_state)
                .with_upstream_registry(upstream_registry);

            if let Some(backend) = inference_backend {
                app_state = app_state.with_inference_backend(backend);
            }

            app_state
        } else {
            info!("WebAuthn is disabled, using JWT authentication only");

            // Use the WebAuthn backend provided to the builder (won't be used when WebAuthn is disabled)
            let dummy_webauthn_backend = self.webauthn_backend.clone();

            // Create a minimal auth service without WebAuthn
            let auth_service = Arc::new(AuthService::new(
                jwt_service.clone(),
                self.state_backend.clone(),
                dummy_webauthn_backend.clone(),
            ));

            // Use the bootstrap manager from builder (required)
            let bootstrap_manager = self
                .bootstrap_manager
                .clone()
                .expect("Bootstrap manager must be provided to ServerBuilder");

            // Create permission manager
            let permission_manager =
                Arc::new(LocalPermissionManager::new(self.state_backend.clone()));

            // Create server state with services
            let server_state = ServerState {
                auth_service,
                webauthn_service: None,
                jwt_service,
                settings: self.settings_arc.clone(),
                bootstrap_manager,
                inference_service: inference_backend.clone(),
                permission_manager,
            };

            let mut app_state = AppState::new(self.state_backend.clone(), server_state)
                .with_upstream_registry(upstream_registry);

            if let Some(backend) = inference_backend {
                app_state = app_state.with_inference_backend(backend);
            }

            app_state
        };

        Ok(state)
    }

    /// Get the WebAuthn service for relay monitoring
    pub fn get_webauthn_service(state: &AppState<ServerState>) -> Arc<WebAuthnService> {
        state.data.webauthn_service.clone().expect("REASON")
    }

    /// Build the complete axum router with documentation
    pub fn build_axum_router<T>(
        router: OpenApiRouter<AppState<T>>,
        state: AppState<T>,
        static_dir: Option<String>,
    ) -> axum::Router
    where
        T: Clone + Send + Sync + 'static + AsRef<Arc<gate_http::services::AuthService>>,
    {
        let (router, api) = router.split_for_parts();

        // Return the configured router with docs at /docs/
        let mut router = router.merge(Scalar::with_url("/docs/", api));

        // Serve static files if configured
        if let Some(static_dir) = static_dir {
            // Check if the static directory exists
            let static_path = std::path::Path::new(&static_dir);
            if static_path.exists() {
                info!("Serving static files from: {}", static_dir);

                // List contents of static directory for debugging
                if let Ok(entries) = std::fs::read_dir(&static_dir) {
                    debug!("Static directory contents:");
                    for entry in entries.flatten() {
                        debug!("  - {}", entry.file_name().to_string_lossy());
                    }
                }

                // Create static file serving with index fallback
                let static_index = std::env::var("GATE_SERVER__STATIC_INDEX")
                    .unwrap_or_else(|_| "index.html".to_string());
                let index_path = static_path.join(&static_index);

                debug!("Index file path: {}", index_path.display());
                debug!("Index file exists: {}", index_path.exists());

                // Create a custom service that logs requests
                let serve_dir =
                    ServeDir::new(&static_dir).not_found_service(if index_path.exists() {
                        tower_http::services::ServeFile::new(index_path)
                    } else {
                        // If index doesn't exist, just return 404
                        tower_http::services::ServeFile::new("/dev/null")
                    });

                // Clone static_dir for use in closure
                let static_dir_for_logging = static_dir.clone();

                // Wrap with logging middleware
                let logged_serve = ServiceBuilder::new()
                    .map_request(move |req: axum::http::Request<_>| {
                        let path = req.uri().path();
                        debug!(
                            "Static file request: {} {} (looking in: {})",
                            req.method(),
                            path,
                            static_dir_for_logging
                        );

                        // Check if the requested file exists
                        let file_path = std::path::Path::new(&static_dir_for_logging)
                            .join(path.trim_start_matches('/'));
                        debug!("Checking for file at: {}", file_path.display());
                        if file_path.exists() {
                            debug!("File exists!");
                        } else {
                            debug!("File not found!");
                        }

                        req
                    })
                    .service(serve_dir);

                router = router.fallback_service(logged_serve);
            } else {
                warn!(
                    "Static directory '{}' does not exist, skipping static file serving",
                    static_dir
                );
            }
        }

        // Convert to regular Axum router with middleware
        // Note: Correlation middleware is applied after building this router to avoid type issues
        router
            .with_state(state.clone())
            .route_layer(axum::middleware::from_fn_with_state(
                state.clone(),
                gate_http::middleware::auth::auth_middleware::<T>,
            ))
            .layer(
                CorsLayer::new()
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config as daemon_config;

    fn test_settings() -> Settings {
        Settings {
            server: daemon_config::ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 0,
                cors_origins: vec![],
                metrics_port: None,
            },
            auth: daemon_config::AuthConfig {
                jwt: daemon_config::JwtConfig {
                    secret: "test-secret".to_string(),
                    expiration_hours: 24,
                    issuer: "gate-test".to_string(),
                },
                webauthn: daemon_config::WebAuthnConfig {
                    enabled: false,
                    rp_id: "localhost".to_string(),
                    rp_name: "Gate Test".to_string(),
                    rp_origin: "http://localhost".to_string(),
                    allowed_origins: vec![],
                    allow_tlsforward_origins: false,
                    allow_subdomains: false,
                    require_user_verification: false,
                    session_timeout_seconds: 3600,
                },
                registration: daemon_config::RegistrationConfig {
                    allow_open_registration: true,
                },
            },
            upstreams: vec![],
            tlsforward: daemon_config::TlsForwardConfig::default(),
            letsencrypt: daemon_config::LetsEncryptConfig::default(),
            local_inference: None,
        }
    }

    #[tokio::test]
    async fn test_router_builds_without_panic() {
        // This test ensures the router can be built without overlapping routes
        let mut router = gate_http::routes::router();

        // Add all route modules
        router = gate_http::routes::dashboard::add_routes(router);
        router = gate_http::routes::inference::add_routes(router);
        router = gate_http::routes::models::add_routes(router);
        router = gate_http::routes::observability::add_routes(router);

        // Add daemon-specific routes
        router = crate::routes::config::add_routes(router);
        router = crate::routes::auth::add_routes(router);
        let _router = crate::routes::admin::add_routes(router);

        // If we get here without panic, the test passes
    }

    #[tokio::test]
    async fn test_server_builder_creates_valid_state() {
        let settings = test_settings();

        // Create in-memory SQLite database for testing
        let state_backend = Arc::new(
            gate_sqlx::SqliteStateBackend::new(":memory:")
                .await
                .unwrap(),
        );
        let webauthn_backend = Arc::new(gate_sqlx::SqlxWebAuthnBackend::new(
            state_backend.pool().clone(),
        ));

        let settings_arc = Arc::new(settings.clone());
        let mut builder =
            ServerBuilder::new(settings, state_backend, webauthn_backend, settings_arc);

        // Build JWT service
        let jwt_service = builder.build_jwt_service();
        // JWT service was created successfully

        let bootstrap_manager =
            Arc::new(BootstrapTokenManager::new(builder.webauthn_backend.clone()));
        builder = builder.with_bootstrap_manager(bootstrap_manager);

        // Build upstream registry
        let upstream_registry = builder.build_upstream_registry().await.unwrap();
        // Upstream registry was created successfully

        // Build app state
        let _app_state = builder
            .build_app_state(jwt_service, upstream_registry)
            .await
            .unwrap();
    }
}
