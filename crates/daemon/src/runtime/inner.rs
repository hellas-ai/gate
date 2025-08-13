use anyhow::{Result, anyhow};
use gate_core::{
    BootstrapTokenValidator, WebAuthnBackend,
    tracing::{metrics, prometheus::export_prometheus},
};
use gate_http::{AppState, server::HttpServer};
use gate_sqlx::{SqliteStateBackend, SqlxWebAuthnBackend};
use std::sync::Arc;
use tokio::{net::TcpListener, sync::watch, task::JoinHandle};
use tracing::{debug, error, info};

use crate::{
    Settings, StateDir,
    bootstrap::BootstrapTokenManager,
    runtime::helpers::*,
    server::ServerBuilder,
    services::{
        TlsForwardService,
        monitoring::MonitoringService,
        p2p::{P2PConfig, P2PManager},
        tls::TlsManager,
    },
};

/// Internal runtime state containing all components
pub(super) struct RuntimeInner {
    pub settings: Settings,
    pub app_state: AppState<crate::ServerState>,
    pub axum_app: axum::Router,
    pub tlsforward_service: Option<Arc<TlsForwardService>>,
    pub bootstrap_token: Option<String>,
    pub user_count: usize,
}

impl RuntimeInner {
    /// Initialize all runtime components
    pub async fn initialize(
        settings: Settings,
        state_dir: StateDir,
        database_url: String,
        static_dir: Option<String>,
    ) -> Result<Self> {
        debug!("Initializing runtime components");

        // Create database backend
        let state_backend = Arc::new(SqliteStateBackend::new(&database_url).await?);
        let webauthn_backend = Arc::new(SqlxWebAuthnBackend::new(state_backend.pool().clone()));
        debug!("Connected to database");

        // Check bootstrap and count users
        let bootstrap_manager = Arc::new(BootstrapTokenManager::new(webauthn_backend.clone()));

        // Count existing users
        let credentials = webauthn_backend
            .list_all_credentials()
            .await
            .map_err(|e| anyhow!("Failed to list credentials: {}", e))?;
        let user_count = credentials.len();

        let bootstrap_token = if bootstrap_manager
            .needs_bootstrap()
            .await
            .map_err(|e| anyhow!(e))?
        {
            let token = bootstrap_manager
                .generate_token()
                .await
                .map_err(|e| anyhow!(e))?;
            info!("Generated bootstrap token: {}", token);
            Some(token)
        } else {
            None
        };

        // Setup TLS if needed
        let tls_manager = if settings.tlsforward.enabled || settings.letsencrypt.enabled {
            let initial_domains = if settings.letsencrypt.enabled {
                settings.letsencrypt.domains.clone()
            } else {
                vec![]
            };

            Some(TlsManager::new(&state_dir.data_dir(), initial_domains).await?)
        } else {
            None
        };

        // Build app state
        let settings_arc = Arc::new(settings.clone());
        let builder = ServerBuilder::new(
            settings.clone(),
            state_backend.clone(),
            webauthn_backend,
            settings_arc.clone(),
        )
        .with_bootstrap_manager(bootstrap_manager.clone());

        let jwt_service = builder.build_jwt_service();
        let upstream_registry = builder.build_upstream_registry().await?;
        let app_state = builder
            .build_app_state(jwt_service, upstream_registry)
            .await?;

        // Build router
        let router = build_daemon_router();

        // Build axum app
        let axum_app = ServerBuilder::build_axum_router(router, app_state.clone(), static_dir)
            .layer(axum::middleware::from_fn(
                gate_http::middleware::correlation_id_middleware,
            ));

        // Create HTTP server
        let http_server = Arc::new(HttpServer::new(axum_app.clone()));

        // Setup P2P if TLS forward is enabled
        let (_p2p_manager, tlsforward_service) = if settings.tlsforward.enabled {
            let p2p_config = P2PConfig {
                secret_key_path: state_dir.iroh_secret_key_path(),
                enable_discovery: false,
                tlsforward_addresses: settings.tlsforward.tlsforward_addresses.clone(),
            };

            let mut p2p_manager = P2PManager::new(p2p_config).await?;

            // Setup TLS forward handler if we have a TLS manager
            if let Some(tls_mgr) = &tls_manager {
                p2p_manager.setup_tls_forward_handler(
                    tls_mgr.acceptor(),
                    http_server.clone(),
                    100, // max_connections
                    30,  // connection_timeout_secs
                )?;
            }

            // Start TLS forward client service
            let mut tlsforward_config = settings.tlsforward.clone();
            if tlsforward_config.secret_key_path.is_none() {
                tlsforward_config.secret_key_path = Some(
                    state_dir
                        .iroh_secret_key_path()
                        .to_string_lossy()
                        .into_owned(),
                );
            }

            let service = p2p_manager
                .start_tlsforward_service(tlsforward_config)
                .await?;

            // Setup certificate manager client if Let's Encrypt is enabled
            // This is done asynchronously to avoid blocking startup
            if settings.letsencrypt.enabled {
                let p2p_mgr = p2p_manager.clone();
                let svc = service.clone();
                let tls_mgr_opt = tls_manager.clone();
                tokio::spawn(async move {
                    if let Some(node_id) = p2p_mgr.wait_for_tlsforward_connection(&svc, 30).await
                        && let Some(tls_mgr) = tls_mgr_opt
                    {
                        tls_mgr
                            .set_tls_forward_client(p2p_mgr.endpoint(), node_id)
                            .await;
                        info!("Certificate manager client configured with TLS forward");
                    }
                });
            }

            (Some(p2p_manager), Some(service))
        } else {
            (None, None)
        };

        // Setup monitoring for TLS forward and WebAuthn
        if let (Some(service), Some(tls_mgr)) = (&tlsforward_service, &tls_manager) {
            if settings.auth.webauthn.enabled
                && settings.auth.webauthn.allow_tlsforward_origins
                && let Some(webauthn_service) =
                    ServerBuilder::get_webauthn_service(&app_state).into()
            {
                spawn_webauthn_monitor(service.clone(), webauthn_service).await;
            }

            if settings.letsencrypt.enabled {
                tls_mgr
                    .monitor_tlsforward_certificates(
                        service.clone(),
                        settings.letsencrypt.domains.clone(),
                    )
                    .await;
            }
        }

        // Request initial certificates
        if settings.letsencrypt.enabled && settings.letsencrypt.email.is_some() {
            let email = settings.letsencrypt.email.as_ref().unwrap();
            let mut domains = settings.letsencrypt.domains.clone();

            // Add TLS forward domain if connected
            if let Some(service) = &tlsforward_service {
                let state = service.subscribe().borrow().clone();
                if let crate::services::TlsForwardState::Connected {
                    assigned_domain, ..
                } = state
                    && !domains.contains(&assigned_domain)
                {
                    info!(
                        "Adding TLS forward-assigned domain to Let's Encrypt: {}",
                        assigned_domain
                    );
                    domains.push(assigned_domain);
                }
            }

            if let Some(tls_mgr) = &tls_manager {
                tls_mgr.request_certificates(domains, email).await?;
            }
        }

        Ok(Self {
            settings,
            app_state,
            axum_app,
            tlsforward_service,
            bootstrap_token,
            user_count,
        })
    }

    /// Start the HTTP server
    pub async fn serve(&self, mut shutdown_rx: watch::Receiver<bool>) -> Result<()> {
        let addr = format!(
            "{}:{}",
            self.settings.server.host, self.settings.server.port
        );
        let listener = TcpListener::bind(&addr).await?;
        info!("Server listening on http://{}", addr);

        axum::serve(listener, self.axum_app.clone())
            .with_graceful_shutdown(async move {
                shutdown_rx.changed().await.ok();
            })
            .await?;

        Ok(())
    }

    /// Start monitoring tasks
    pub async fn start_monitoring(&self) -> Vec<JoinHandle<()>> {
        let mut monitoring = MonitoringService::new();

        // Monitor database pool
        monitoring.monitor_database_pool(self.app_state.state_backend.clone());

        // Monitor WebAuthn + TLS forward
        if let Some(service) = &self.tlsforward_service
            && self.settings.auth.webauthn.enabled
            && self.settings.auth.webauthn.allow_tlsforward_origins
            && let Some(webauthn_service) =
                ServerBuilder::get_webauthn_service(&self.app_state).into()
        {
            monitoring.monitor_webauthn_tlsforward(service.clone(), webauthn_service);
        }

        vec![] // Return task handles if needed
    }

    /// Start metrics server
    pub async fn start_metrics(&self) -> Result<Option<JoinHandle<()>>> {
        if let Some(metrics_port) = self.settings.server.metrics_port {
            let metrics_addr = format!("{}:{}", self.settings.server.host, metrics_port);
            info!("Starting metrics server on http://{}/metrics", metrics_addr);

            let metrics_router = axum::Router::new().route(
                "/metrics",
                axum::routing::get(|| async { export_prometheus(metrics::global()) }),
            );

            let metrics_listener = TcpListener::bind(&metrics_addr).await?;

            Ok(Some(tokio::spawn(async move {
                if let Err(e) = axum::serve(metrics_listener, metrics_router).await {
                    error!("Metrics server error: {}", e);
                }
            })))
        } else {
            info!("Metrics server not configured (set server.metrics_port to enable)");
            Ok(None)
        }
    }
}
