use anyhow::anyhow;
use anyhow::{Context, Result};
use clap::Parser;
use gate_core::BootstrapTokenValidator;
use gate_core::tracing::{
    config::{InstrumentationConfig, OtlpConfig},
    init::init_tracing,
    metrics,
    prometheus::export_prometheus,
};
use gate_daemon::bootstrap::BootstrapTokenManager;
use gate_daemon::{
    Settings, StateDir, server::ServerBuilder, services::TlsForwardService,
    tls_reload::ReloadableTlsAcceptor,
};
use gate_sqlx::SqliteStateBackend;
use gate_tlsforward::{CertificateManager, TLS_FORWARD_ALPN};
use iroh::{NodeAddr, discovery::static_provider::StaticProvider, protocol::Router};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tracing::{debug, error, info, warn};

/// Gate daemon - High-performance AI gateway
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Configuration file path
    #[arg(short = 'c', long = "config")]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install the default crypto provider for rustls
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Parse command line arguments
    let cli = Cli::parse();

    // Load environment variables from .env file
    dotenvy::dotenv().ok();

    // Initialize instrumentation
    let instrumentation_config = InstrumentationConfig {
        service_name: "gate-daemon".to_string(),
        service_version: env!("CARGO_PKG_VERSION").to_string(),
        log_level: std::env::var("RUST_LOG")
            .unwrap_or_else(|_| "gate=debug,tower_http=debug".to_string()),
        otlp: std::env::var("OTLP_ENDPOINT")
            .ok()
            .map(|endpoint| OtlpConfig {
                endpoint,
                headers: None,
            }),
    };
    init_tracing(&instrumentation_config)?;

    // Create state directory manager
    let state_dir = StateDir::new();
    let config_path = state_dir.config_path();

    // Load configuration with startup and runtime config support
    let settings = if let Some(config_path) = &cli.config {
        // Use explicit config path
        info!("Loading configuration from: {}", config_path);
        Settings::load_from_file(config_path)
            .inspect_err(|e| println!("Error loading config from {config_path}: {e}"))?
    } else if config_path.exists() {
        // Use startup config from state directory
        info!(
            "Loading startup configuration from: {}",
            config_path.display()
        );
        Settings::load_from_file(&config_path.to_string_lossy()).inspect_err(|e| {
            println!(
                "Error loading config from {}: {e}",
                &config_path.to_string_lossy()
            )
        })?
    } else {
        // Fall back to default config
        info!("Didn't find config file, using default settings");
        Settings::default()
    };
    let settings_arc = Arc::new(settings.clone());

    debug!("Starting Gate Daemon with configuration: {:#?}", settings);
    // Create directories
    state_dir.create_directories().await?;

    // Create state backend
    let database_url = format!(
        "sqlite://{}",
        state_dir.data_dir().join("gate.db").display()
    );
    let state_backend = Arc::new(SqliteStateBackend::new(&database_url).await?);
    debug!("Connected to database");

    // Create WebAuthn backend using the same pool
    let webauthn_backend = Arc::new(gate_sqlx::SqlxWebAuthnBackend::new(
        state_backend.pool().clone(),
    ));

    // Generate + print bootstrap token if needed
    let bootstrap_manager = Arc::new(BootstrapTokenManager::new(webauthn_backend.clone()));
    if bootstrap_manager
        .needs_bootstrap()
        .await
        .map_err(|e| anyhow!(e))?
    {
        let token = bootstrap_manager
            .generate_token()
            .await
            .map_err(|e| anyhow!(e))?;
        println!("Bootstrap token: {token}");
    }

    // Create certificate manager with platform-specific data directory
    let certificate_manager = Arc::new(tokio::sync::Mutex::new(CertificateManager::new(
        state_dir.data_dir(),
    )));

    // Create reloadable TLS acceptor from certificates
    let reloadable_tls_acceptor = {
        let cert_mgr = certificate_manager.lock().await;
        let mut domains = Vec::new();

        // Add Let's Encrypt domains first (if any)
        if settings.letsencrypt.enabled && !settings.letsencrypt.domains.is_empty() {
            domains.extend(settings.letsencrypt.domains.clone());
        }

        // Only add localhost if no other domains are configured
        if domains.is_empty() {
            domains.push("localhost".to_string());
        }

        let acceptor = cert_mgr.get_or_create_tls_acceptor(&domains).await?;
        ReloadableTlsAcceptor::new(acceptor)
    };

    // Create server builder
    let server_builder = ServerBuilder::new(
        settings.clone(),
        state_backend.clone(),
        webauthn_backend.clone(),
        settings_arc.clone(),
    );

    // Build services
    let jwt_service = server_builder.build_jwt_service();
    let upstream_registry = server_builder.build_upstream_registry().await?;

    // Prepare TLS forward config if enabled
    let tlsforward_config = if settings.tlsforward.enabled {
        let mut tlsforward_config = settings.tlsforward.clone();

        // Set secret key path if not explicitly configured
        if tlsforward_config.secret_key_path.is_none() {
            tlsforward_config.secret_key_path = Some(
                state_dir
                    .iroh_secret_key_path()
                    .to_string_lossy()
                    .into_owned(),
            );
        }

        Some(tlsforward_config)
    } else {
        warn!("TLS forward service is disabled: {:?}", settings.tlsforward);
        None
    };

    // Build app state
    let state = server_builder
        .build_app_state(jwt_service.clone(), upstream_registry.clone())
        .await?;

    // Store WebAuthn service reference for TLS forward monitoring if enabled
    let webauthn_service_for_tlsforward = Arc::new(tokio::sync::RwLock::new(None));
    if settings.auth.webauthn.enabled {
        *webauthn_service_for_tlsforward.write().await =
            Some(ServerBuilder::get_webauthn_service(&state));
    }

    // Build router
    let mut router = gate_http::routes::router();

    // Add all route modules
    router = gate_http::routes::dashboard::add_routes(router);
    router = gate_http::routes::inference::add_routes(router);
    router = gate_http::routes::models::add_routes(router);
    router = gate_http::routes::observability::add_routes(router);

    // Add daemon-specific routes
    router = gate_daemon::routes::config::add_routes(router);
    router = gate_daemon::routes::auth::add_routes(router);
    router = gate_daemon::routes::admin::add_routes(router);

    // Get static directory from environment
    let static_dir = std::env::var("GATE_SERVER__STATIC_DIR")
        .unwrap_or_else(|_| "crates/frontend-daemon/dist".to_string());

    // Build the complete axum router
    let app = ServerBuilder::build_axum_router(router, state.clone(), Some(static_dir));

    // Apply correlation ID middleware
    let app = app.layer(axum::middleware::from_fn(
        gate_http::middleware::correlation_id_middleware,
    ));

    // Create HTTP server for stream handling
    let http_server = Arc::new(gate_http::server::HttpServer::new(app.clone()));

    // Create P2P endpoint and router if TLS forward is enabled
    let (p2p_endpoint, _p2p_router) = if tlsforward_config.is_some() {
        debug!("Creating P2P endpoint for TLS forward service");

        // Load or create P2P secret key
        let secret_key_path = state_dir.iroh_secret_key_path();
        let secret_key = load_or_create_p2p_secret_key(&secret_key_path).await?;

        // Create P2P endpoint with static discovery for TLS forward servers
        let mut endpoint_builder = gate_p2p::Endpoint::builder().secret_key(secret_key);

        // If we have TLS forward addresses configured, add them to static discovery
        if let Some(ref config) = tlsforward_config {
            let static_provider = StaticProvider::new();

            // Parse and add each TLS forward address to static discovery
            for tlsforward_addr_str in &config.tlsforward_addresses {
                if let Some((node_id_str, addr_str)) = tlsforward_addr_str.split_once('@') {
                    // Format: node_id@address:port
                    if let Ok(node_id) = node_id_str.parse::<iroh::NodeId>()
                        && let Ok(socket_addr) = addr_str.parse::<std::net::SocketAddr>()
                    {
                        let node_addr = NodeAddr::from_parts(
                            node_id,
                            None, // No relay URL
                            vec![socket_addr],
                        );
                        static_provider.add_node_info(node_addr);
                        info!(
                            "Added static node discovery for TLS forward server: {} at {}",
                            node_id, socket_addr
                        );
                    }
                }
            }

            endpoint_builder = endpoint_builder.add_discovery(static_provider);
        }

        // Create P2P endpoint
        match endpoint_builder.bind().await {
            Ok(endpoint) => {
                info!("P2P endpoint created with node ID: {}", endpoint.node_id());

                // Create TLS forward handler to accept incoming connections
                let tls_handler = gate_tlsforward::TlsForwardHandler::new(
                    reloadable_tls_acceptor.clone(),
                    http_server.clone(),
                    100, // max_connections
                    30,  // connection_timeout_secs
                );

                // Create router and register the TLS forward handler
                let router = Router::builder(endpoint.clone())
                    .accept(TLS_FORWARD_ALPN, tls_handler)
                    .spawn();

                debug!("Registered TLS forward handler on P2P endpoint");

                (Some(Arc::new(endpoint)), Some(router))
            }
            Err(e) => {
                error!("Failed to create P2P endpoint: {}", e);
                return Err(anyhow::anyhow!("Failed to create P2P endpoint: {}", e));
            }
        }
    } else {
        (None, None)
    };

    // Create TLS forward service if enabled (for connecting to relay server)
    let tlsforward_service = if let Some(config) = tlsforward_config {
        debug!("Initializing TLS forward client service");

        let endpoint = p2p_endpoint
            .as_ref()
            .expect("P2P endpoint must exist for TLS forward");

        let tlsforward_service = TlsForwardService::builder(config, endpoint.clone())
            .build()
            .await?;

        // Setup certificate manager with TLS forward server if Let's Encrypt is enabled
        if settings.letsencrypt.enabled {
            // Wait for TLS forward server to connect
            let mut retry_count = 0;
            let tlsforward_node_id = loop {
                if let Some(node_id) = tlsforward_service.tlsforward_node_id().await {
                    break Some(node_id);
                }
                retry_count += 1;
                if retry_count > 30 {
                    warn!(
                        "TLS forward server not connected after 30 seconds, Let's Encrypt will not be available"
                    );
                    break None;
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            };

            if let Some(tlsforward_node_id) = tlsforward_node_id {
                // Use our P2P endpoint
                let endpoint = p2p_endpoint.as_ref().expect("P2P endpoint must exist");
                let tls_forward_client =
                    gate_tlsforward::TlsForwardClient::new(endpoint.clone(), tlsforward_node_id);
                certificate_manager
                    .lock()
                    .await
                    .set_tls_forward_client(tls_forward_client);
                debug!("Certificate manager configured with TLS forward client");
            }
        }

        // Monitor TLS forward state changes for WebAuthn configuration updates
        if settings.auth.webauthn.enabled && settings.auth.webauthn.allow_tlsforward_origins {
            let tlsforward_state_rx = tlsforward_service.subscribe();
            let webauthn_service_ref = webauthn_service_for_tlsforward.clone();

            tokio::spawn(async move {
                let mut tlsforward_state_rx = tlsforward_state_rx;
                let mut last_domain: Option<String> = None;

                loop {
                    if tlsforward_state_rx.changed().await.is_err() {
                        break;
                    }

                    let state = tlsforward_state_rx.borrow().clone();
                    if let gate_daemon::services::TlsForwardState::Connected {
                        assigned_domain,
                        ..
                    } = state
                    {
                        // Check if domain changed
                        if last_domain.as_ref() != Some(&assigned_domain) {
                            // Get the webauthn service reference
                            if let Some(webauthn_service) =
                                webauthn_service_ref.read().await.as_ref()
                            {
                                // Build the HTTPS origin URL for the assigned domain
                                let tlsforward_origin = format!("https://{assigned_domain}");

                                info!(
                                    "TLS forward connected with domain: {}, updating WebAuthn allowed origins",
                                    assigned_domain
                                );

                                // Add the TLS forward origin to WebAuthn allowed origins
                                if let Err(e) = webauthn_service
                                    .add_allowed_origin(tlsforward_origin.clone())
                                    .await
                                {
                                    error!("Failed to add TLS forward origin to WebAuthn: {}", e);
                                } else {
                                    debug!(
                                        "Successfully added {} to WebAuthn allowed origins",
                                        tlsforward_origin
                                    );
                                    last_domain = Some(assigned_domain);
                                }
                            }
                        }
                    }
                }
            });
        }

        // Monitor TLS forward state changes for TLS certificate updates
        if settings.letsencrypt.enabled {
            let tlsforward_state_rx = tlsforward_service.subscribe();
            let certificate_manager_clone = certificate_manager.clone();
            let reloadable_tls_acceptor_clone = reloadable_tls_acceptor.clone();
            let letsencrypt_domains = settings.letsencrypt.domains.clone();

            tokio::spawn(async move {
                let mut tlsforward_state_rx = tlsforward_state_rx;
                let mut last_domain: Option<String> = None;

                loop {
                    if tlsforward_state_rx.changed().await.is_err() {
                        break;
                    }

                    let state = tlsforward_state_rx.borrow().clone();
                    if let gate_daemon::services::TlsForwardState::Connected {
                        assigned_domain, ..
                    } = state
                        && last_domain.as_ref() != Some(&assigned_domain)
                    {
                        info!("TLS forward connected with new domain: {}", assigned_domain);

                        // Configuration updates removed - TLS forward domain changes require restart
                        info!(
                            "TLS forward domain {} detected. Manual config update required for Let's Encrypt.",
                            assigned_domain
                        );

                        // Check if we have a certificate for this domain
                        let cert_mgr = certificate_manager_clone.lock().await;
                        if cert_mgr.has_certificate(&assigned_domain).await {
                            info!(
                                "Found existing certificate for TLS forward domain: {}",
                                assigned_domain
                            );

                            // Reload TLS acceptor with TLS forward domain
                            let mut domains = vec![assigned_domain.clone()];
                            domains.extend(letsencrypt_domains.clone());

                            if let Ok(new_acceptor) =
                                cert_mgr.get_or_create_tls_acceptor(&domains).await
                            {
                                reloadable_tls_acceptor_clone.reload(new_acceptor).await;
                                info!(
                                    "Reloaded TLS acceptor with TLS forward domain: {}",
                                    assigned_domain
                                );
                                last_domain = Some(assigned_domain);
                            }
                        } else {
                            info!(
                                "No certificate found for TLS forward domain: {}, will be requested later",
                                assigned_domain
                            );
                            last_domain = Some(assigned_domain);
                        }
                    }
                }
            });
        }

        Some(tlsforward_service)
    } else {
        None
    };

    // Start server
    let addr = format!("{}:{}", settings.server.host, settings.server.port);
    let listener = TcpListener::bind(&addr).await?;
    info!("Server listening on http://{}", addr);

    // Handle graceful shutdown
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    // Spawn server task
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown_rx.changed().await.ok();
            })
            .await
    });

    // Start metrics server if configured
    let metrics_handle = if let Some(metrics_port) = settings.server.metrics_port {
        let metrics_addr = format!("{}:{}", settings.server.host, metrics_port);
        info!("Starting metrics server on http://{}/metrics", metrics_addr);

        let metrics_router = axum::Router::new().route(
            "/metrics",
            axum::routing::get(|| async { export_prometheus(metrics::global()) }),
        );

        let metrics_listener = TcpListener::bind(&metrics_addr).await?;

        Some(tokio::spawn(async move {
            if let Err(e) = axum::serve(metrics_listener, metrics_router).await {
                error!("Metrics server error: {}", e);
            }
        }))
    } else {
        info!("Metrics server not configured (set server.metrics_port to enable)");
        None
    };

    // Start database pool metrics collection
    let pool_metrics_backend = state_backend.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;

            // Collect and record database pool metrics
            let (active, idle, total) = pool_metrics_backend.pool_metrics();

            // Also record to simple metrics
            metrics::gauge("database_connections_active").set(active as i64);
            metrics::gauge("database_connections_idle").set(idle as i64);
            metrics::gauge("database_connections_total").set(total as i64);

            tracing::debug!(
                active = active,
                idle = idle,
                total = total,
                "Database pool metrics collected"
            );

            // TLS forward connection pool has been removed in v0.90.0 migration
        }
    });

    // Request certificates if Let's Encrypt is enabled
    if settings.letsencrypt.enabled {
        // Get domains to request certificates for
        let mut domains = settings.letsencrypt.domains.clone();

        // Add TLS forward-assigned domain if connected
        if let Some(tlsforward_service) = &tlsforward_service {
            let state = tlsforward_service.subscribe().borrow().clone();
            if let gate_daemon::services::TlsForwardState::Connected {
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

        if !domains.is_empty() {
            let email = settings.letsencrypt.email.clone().unwrap_or_else(|| {
                warn!("No email configured for Let's Encrypt, using default");
                "admin@example.com".to_string()
            });

            info!("Requesting certificates for configured domains");
            for domain in &domains {
                info!("Checking certificate for domain: https://{}", domain);

                let cert_mgr = certificate_manager.lock().await;
                if !cert_mgr.has_certificate(domain).await {
                    info!("Requesting new certificate for https://{}", domain);
                    match cert_mgr.request_certificate(domain, &email).await {
                        Ok(()) => {
                            info!("Successfully obtained certificate for {}", domain);
                            // Reload TLS acceptor with new certificates
                            // Use only the Let's Encrypt domains (no localhost fallback)
                            if let Ok(new_acceptor) =
                                cert_mgr.get_or_create_tls_acceptor(&domains).await
                            {
                                reloadable_tls_acceptor.reload(new_acceptor).await;
                                info!(
                                    "Reloaded TLS acceptor with new certificates for domains: {:?}",
                                    domains
                                );
                            }
                        }
                        Err(e) => {
                            error!("Failed to obtain certificate for https://{}: {}", domain, e)
                        }
                    }
                } else {
                    info!("Certificate already exists for https://{}", domain);
                }
            }
        }
    }

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;
    info!("Received shutdown signal");

    // Signal shutdown
    shutdown_tx.send(true)?;

    // Shutdown TLS forward service
    if let Some(tlsforward_service) = tlsforward_service {
        info!("Shutting down TLS forward service");
        tlsforward_service.shutdown().await;
    }

    // Wait for server to shutdown
    server_handle.await??;

    // Shutdown metrics server if running
    if let Some(handle) = metrics_handle {
        handle.abort();
    }

    Ok(())
}

/// Load or create P2P secret key
async fn load_or_create_p2p_secret_key(path: &std::path::Path) -> Result<gate_p2p::SecretKey> {
    use tokio::fs;

    if path.exists() {
        // Try to load existing key
        match fs::read_to_string(&path).await {
            Ok(contents) => {
                let hex_key = contents.trim();
                match hex::decode(hex_key) {
                    Ok(key_bytes) if key_bytes.len() == 32 => {
                        let mut key_array = [0u8; 32];
                        key_array.copy_from_slice(&key_bytes);
                        let secret_key = gate_p2p::SecretKey::from_bytes(&key_array);
                        info!("Loaded P2P secret key from {}", path.display());
                        Ok(secret_key)
                    }
                    _ => {
                        warn!(
                            "Invalid P2P secret key format in {}, generating new key",
                            path.display()
                        );
                        create_and_save_p2p_key(path).await
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Failed to read P2P secret key from {}: {}, generating new key",
                    path.display(),
                    e
                );
                create_and_save_p2p_key(path).await
            }
        }
    } else {
        info!(
            "No P2P secret key found at {}, generating new key",
            path.display()
        );
        create_and_save_p2p_key(path).await
    }
}

async fn create_and_save_p2p_key(path: &std::path::Path) -> Result<gate_p2p::SecretKey> {
    use rand::rngs::OsRng;
    use tokio::fs;

    // Generate a random secret key
    let secret_key = gate_p2p::SecretKey::generate(OsRng);
    let hex_key = hex::encode(secret_key.to_bytes());

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    // Write key to file
    fs::write(path, hex_key)
        .await
        .with_context(|| format!("Failed to write P2P secret key to {}", path.display()))?;

    info!(
        "Generated and saved new P2P secret key to {}",
        path.display()
    );
    Ok(secret_key)
}
