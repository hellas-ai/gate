use crate::bootstrap::BootstrapTokenManager;
use crate::daemon::{Daemon, actor::DaemonActor, inner::DaemonInner};
use crate::error::Result;
use crate::services::{AuthService, WebAuthnService};
use crate::{Settings, StateDir};
use gate_http::{
    UpstreamRegistry,
    forwarding::ForwardingConfig,
    middleware::WebAuthnConfig,
    model_detection,
    services::{JwtConfig, JwtService},
};
use gate_sqlx::{SqliteStateBackend, SqliteWebAuthnBackend};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Default)]
pub struct DaemonBuilder {
    settings: Option<Settings>,
    state_dir: Option<StateDir>,
    database_url: Option<String>,
    static_dir: Option<String>,
}

impl DaemonBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_settings(mut self, settings: Settings) -> Self {
        self.settings = Some(settings);
        self
    }

    pub fn with_state_dir(mut self, state_dir: StateDir) -> Self {
        self.state_dir = Some(state_dir);
        self
    }

    pub fn with_database_url(mut self, url: String) -> Self {
        self.database_url = Some(url);
        self
    }

    pub fn with_static_dir(mut self, dir: String) -> Self {
        self.static_dir = Some(dir);
        self
    }

    /// Build the JWT service
    fn build_jwt_service(settings: &Settings) -> Arc<JwtService> {
        let jwt_config = JwtConfig {
            secret: settings.auth.jwt.secret.clone(),
            expiration: chrono::Duration::hours(settings.auth.jwt.expiration_hours as i64),
            issuer: settings.auth.jwt.issuer.clone(),
        };
        Arc::new(JwtService::new(jwt_config))
    }

    /// Build the upstream registry
    async fn build_upstream_registry(settings: &Settings) -> Result<Arc<UpstreamRegistry>> {
        let upstream_registry = Arc::new(UpstreamRegistry::new());

        if !settings.upstreams.is_empty() {
            info!(
                "Initializing {} upstream providers",
                settings.upstreams.len()
            );

            for upstream_config in &settings.upstreams {
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

    pub async fn build(self) -> Result<Daemon> {
        // Get or create state directory
        let state_dir = self
            .state_dir
            .ok_or(crate::error::DaemonError::ConfigError(
                "State directory not set".to_string(),
            ))?;

        // Get or create settings
        let settings = if let Some(settings) = self.settings {
            settings
        } else {
            let config_path = state_dir.config_path();
            if config_path.exists() {
                info!("Loading configuration from: {}", config_path.display());
                Settings::load_from_file(config_path)
                    .map_err(|e| crate::error::DaemonError::ConfigError(e.to_string()))?
            } else {
                info!("No config file found, using default settings");
                Settings::default()
            }
        };

        // Get database URL
        let database_url = self.database_url.unwrap_or_else(|| {
            format!(
                "sqlite://{}",
                state_dir.data_dir().join("gate.db").display()
            )
        });

        // Create database backend
        let state_backend = Arc::new(
            SqliteStateBackend::new(&database_url)
                .await
                .map_err(|e| crate::error::DaemonError::Database(e.to_string()))?,
        );
        let webauthn_backend = Arc::new(SqliteWebAuthnBackend::new(state_backend.pool().clone()));

        // Check bootstrap and count users
        let bootstrap_manager = Arc::new(BootstrapTokenManager::new(webauthn_backend.clone()));

        // Count existing users
        let credentials = webauthn_backend.list_all_credentials().await.map_err(|e| {
            crate::error::DaemonError::Database(format!("Failed to list credentials: {e}"))
        })?;
        let user_count = credentials.len();

        if bootstrap_manager
            .needs_bootstrap()
            .await
            .map_err(|e| crate::error::DaemonError::ConfigError(e.to_string()))?
        {
            let token = bootstrap_manager
                .generate_token()
                .await
                .map_err(|e| crate::error::DaemonError::ConfigError(e.to_string()))?;
            info!("Generated bootstrap token: {}", token);
        }

        // Build services
        let jwt_service = Self::build_jwt_service(&settings);
        let upstream_registry = Self::build_upstream_registry(&settings)
            .await
            .map_err(|e| crate::error::DaemonError::ConfigError(e.to_string()))?;

        // Build auth and webauthn services
        let (auth_service, webauthn_service) = {
            // Create WebAuthn configuration and service
            let webauthn_config = WebAuthnConfig {
                rp_id: settings.auth.webauthn.rp_id.clone(),
                rp_name: settings.auth.webauthn.rp_name.clone(),
                rp_origin: settings.auth.webauthn.rp_origin.clone(),
                allowed_origins: settings.auth.webauthn.allowed_origins.clone(),
                allow_tlsforward_origins: settings.auth.webauthn.allow_tlsforward_origins,
                allow_subdomains: settings.auth.webauthn.allow_subdomains,
                require_user_verification: settings.auth.webauthn.require_user_verification,
                session_timeout_seconds: settings.auth.webauthn.session_timeout_seconds,
            };

            debug!(
                "Creating WebAuthn service with RP ID: '{}', RP Origin: '{}', Allow Relay: {}",
                webauthn_config.rp_id,
                webauthn_config.rp_origin,
                webauthn_config.allow_tlsforward_origins
            );

            let webauthn_service = Arc::new(
                WebAuthnService::new(webauthn_config.clone(), webauthn_backend.clone()).map_err(
                    |e| {
                        crate::error::DaemonError::ConfigError(format!(
                            "Failed to create WebAuthn service: {e}",
                        ))
                    },
                )?,
            );

            let auth_service = Arc::new(AuthService::new(
                jwt_service.clone(),
                state_backend.clone(),
                webauthn_backend.clone(),
            ));

            (auth_service, Some(webauthn_service))
        };

        // TODO: Setup TLS forward service if enabled
        let tlsforward_service = None;

        // Setup local inference service if configured
        let inference_backend: Option<Arc<dyn gate_core::InferenceBackend>> =
            if let Some(ref inference_config) = settings.local_inference {
                match crate::services::LocalInferenceService::new(inference_config.clone()) {
                    Ok(service) => {
                        info!("Local inference service initialized");
                        Some(Arc::new(service) as Arc<dyn gate_core::InferenceBackend>)
                    }
                    Err(e) => {
                        warn!("Failed to initialize local inference service: {}", e);
                        None
                    }
                }
            } else {
                None
            };

        // Create DaemonInner
        let daemon_inner = DaemonInner::new(
            settings,
            state_backend,
            auth_service,
            jwt_service,
            bootstrap_manager,
            webauthn_service,
            tlsforward_service,
            upstream_registry,
            inference_backend,
            user_count,
        )
        .await;

        // Create channel for actor communication
        let (tx, rx) = mpsc::channel(100);

        // Spawn actor
        let actor = DaemonActor::new(daemon_inner, rx);
        tokio::spawn(async move {
            actor.run().await;
        });

        // Return daemon handle with static_dir
        Ok(Daemon::new(tx, self.static_dir))
    }
}
