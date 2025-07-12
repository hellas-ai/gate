use anyhow::Result;
use gate_core::BootstrapTokenValidator;
use gate_daemon::{
    Settings, StateDir, server::ServerBuilder, services::tlsforward::TlsForwardService,
    tls_reload::ReloadableTlsAcceptor,
};
use gate_http::{AppState, server::HttpServer};
use gate_p2p::{NodeAddr, Router, discovery::static_provider::StaticProvider};
use gate_sqlx::{SqliteStateBackend, SqlxWebAuthnBackend};
use gate_tlsforward::{CertificateManager, TLS_FORWARD_ALPN, TlsForwardHandler};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tokio::sync::{Mutex, RwLock, watch};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

// Struct to group daemon runtime parameters
struct DaemonRuntimeParams {
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
    runtime_tx: tokio::sync::oneshot::Sender<DaemonRuntime>,
    tlsforward_state_tx: tokio::sync::oneshot::Sender<Option<TlsForwardState>>,
    tlsforward_state: Arc<RwLock<Option<TlsForwardState>>>,
    tlsforward_state_rx:
        Arc<RwLock<Option<watch::Receiver<gate_daemon::services::tlsforward::TlsForwardState>>>>,
}

// Helper functions module
mod helpers {
    use super::*;
    use gate_p2p::{Endpoint, SecretKey};
    use std::path::Path;

    /// Load or create P2P secret key (reusing logic from gate-daemon)
    pub async fn load_or_create_p2p_secret_key(path: &Path) -> Result<SecretKey> {
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
                            let secret_key = SecretKey::from_bytes(&key_array);
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
                "P2P secret key file not found at {}, generating new key",
                path.display()
            );
            create_and_save_p2p_key(path).await
        }
    }

    /// Create and save a new P2P secret key
    async fn create_and_save_p2p_key(path: &Path) -> Result<SecretKey> {
        use rand::rngs::OsRng;
        use tokio::fs;

        let secret_key = SecretKey::generate(OsRng);
        let key_bytes = secret_key.to_bytes();
        let hex_key = hex::encode(key_bytes);

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        fs::write(path, hex_key).await?;
        info!("Created and saved new P2P secret key to {}", path.display());
        Ok(secret_key)
    }

    /// Create P2P endpoint with optional static discovery
    pub async fn create_p2p_endpoint(
        secret_key: SecretKey,
        enable_discovery: bool,
        tlsforward_servers: &[String],
    ) -> Result<Arc<Endpoint>> {
        let mut endpoint_builder = Endpoint::builder().secret_key(secret_key);

        if enable_discovery {
            endpoint_builder = endpoint_builder.discovery_n0();
        }

        // Add static discovery for TLS forward servers if configured
        if !tlsforward_servers.is_empty() {
            let static_provider = StaticProvider::new();

            for server in tlsforward_servers {
                // Parse TLS forward server address
                if let Some((node_id_str, addr_str)) = server.split_once('@')
                    && let Ok(node_id) = node_id_str.parse::<gate_p2p::NodeId>()
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

            endpoint_builder = endpoint_builder.add_discovery(static_provider);
        }

        Ok(Arc::new(endpoint_builder.bind().await?))
    }

    /// Setup certificate manager and TLS acceptor
    pub async fn setup_certificate_manager(
        state_dir: &StateDir,
        enabled: bool,
    ) -> Result<(
        Option<Arc<ReloadableTlsAcceptor>>,
        Option<Arc<Mutex<CertificateManager>>>,
    )> {
        if !enabled {
            return Ok((None, None));
        }

        // Create certificate manager
        let cert_manager = Arc::new(Mutex::new(CertificateManager::new(
            state_dir.data_dir().join("tlsforward/acme_cache"),
        )));

        // Get domains for certificate (start with localhost for initial setup)
        let domains = vec!["localhost".to_string()];

        // Get or create TLS acceptor (initially with self-signed cert)
        let acceptor = cert_manager
            .lock()
            .await
            .get_or_create_tls_acceptor(&domains)
            .await?;
        let reloadable_acceptor = Arc::new(ReloadableTlsAcceptor::new(acceptor));

        Ok((Some(reloadable_acceptor), Some(cert_manager)))
    }

    /// Setup TLS forward monitoring task
    pub async fn spawn_tlsforward_monitor(
        service: Arc<TlsForwardService>,
        cert_manager: Option<Arc<Mutex<CertificateManager>>>,
        reloadable_acceptor: Option<Arc<ReloadableTlsAcceptor>>,
        email: Option<String>,
        endpoint: Arc<Endpoint>,
        tlsforward_state: Arc<RwLock<Option<TlsForwardState>>>,
    ) -> watch::Receiver<gate_daemon::services::tlsforward::TlsForwardState> {
        let state_rx = service.subscribe();
        let mut state_rx_clone = state_rx.clone();

        tokio::spawn(async move {
            let mut last_domain: Option<String> = None;

            while state_rx_clone.changed().await.is_ok() {
                let daemon_state = state_rx_clone.borrow().clone();
                let new_state = convert_tlsforward_state(&daemon_state);

                // Check if we're connected with a new domain
                if let gate_daemon::services::tlsforward::TlsForwardState::Connected {
                    assigned_domain,
                    tlsforward_node,
                    ..
                } = &daemon_state
                {
                    if last_domain.as_ref() != Some(assigned_domain) {
                        info!("TLS forward connected with domain: {}", assigned_domain);

                        // Verify the domain matches our node ID
                        let node_id = endpoint.node_id();
                        let expected_prefix = node_id.fmt_short();
                        let expected_domain = format!("{expected_prefix}.private.hellas.ai");

                        if assigned_domain != &expected_domain {
                            error!(
                                "Domain mismatch! Expected {} based on node ID {}, but got {}",
                                expected_domain, node_id, assigned_domain
                            );
                        }

                        last_domain = Some(assigned_domain.clone());

                        // Request certificate if we have a certificate manager and email
                        if let (Some(cert_mgr), Some(email), Some(acceptor)) = (
                            cert_manager.as_ref(),
                            email.as_ref(),
                            reloadable_acceptor.as_ref(),
                        ) {
                            handle_certificate_request(
                                cert_mgr.clone(),
                                acceptor.clone(),
                                assigned_domain,
                                email,
                                endpoint.clone(),
                                *tlsforward_node,
                                tlsforward_state.clone(),
                                new_state.clone(),
                            )
                            .await;
                        } else {
                            // No certificate manager, just update state
                            *tlsforward_state.write().await = Some(new_state);
                        }
                    }
                } else {
                    // Not connected or same domain, just update state
                    *tlsforward_state.write().await = Some(new_state);
                }
            }
        });

        state_rx
    }

    /// Handle certificate request for assigned domain
    #[allow(clippy::too_many_arguments)]
    async fn handle_certificate_request(
        cert_mgr: Arc<Mutex<CertificateManager>>,
        acceptor: Arc<ReloadableTlsAcceptor>,
        assigned_domain: &str,
        email: &str,
        endpoint: Arc<Endpoint>,
        tlsforward_node: gate_p2p::NodeId,
        tlsforward_state: Arc<RwLock<Option<TlsForwardState>>>,
        new_state: TlsForwardState,
    ) {
        let mut cert_mgr_locked = cert_mgr.lock().await;

        // Ensure the TLS forward client is set up with the connected server
        let tls_forward_client = gate_tlsforward::TlsForwardClient::new(endpoint, tlsforward_node);
        cert_mgr_locked.set_tls_forward_client(tls_forward_client);
        info!("Updated certificate manager with connected TLS forward server");

        if !cert_mgr_locked.has_certificate(assigned_domain).await {
            info!(
                "Requesting Let's Encrypt certificate for: {}",
                assigned_domain
            );

            // Give the server a moment to fully process the domain assignment
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            // Update state to show we're requesting certificate
            *tlsforward_state.write().await = Some(TlsForwardState::Connecting);

            match cert_mgr_locked
                .request_certificate(assigned_domain, email)
                .await
            {
                Ok(()) => {
                    info!("Successfully obtained certificate for {}", assigned_domain);

                    // Reload TLS acceptor with new certificate
                    let domains = vec![assigned_domain.to_string()];
                    match cert_mgr_locked.get_or_create_tls_acceptor(&domains).await {
                        Ok(new_acceptor) => {
                            acceptor.reload(new_acceptor).await;
                            info!(
                                "Reloaded TLS acceptor with certificate for {}",
                                assigned_domain
                            );

                            // Update state to show as connected only after certificate is ready
                            *tlsforward_state.write().await = Some(new_state);
                        }
                        Err(e) => {
                            error!("Failed to reload TLS acceptor: {}", e);
                            *tlsforward_state.write().await = Some(TlsForwardState::Error(
                                format!("Certificate obtained but failed to reload: {e}"),
                            ));
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to request certificate: {}", e);
                    *tlsforward_state.write().await = Some(TlsForwardState::Error(format!(
                        "Failed to obtain certificate: {e}"
                    )));
                }
            }
        } else {
            info!("Certificate already exists for {}", assigned_domain);

            // Reload TLS acceptor with existing certificate
            let domains = vec![assigned_domain.to_string()];
            match cert_mgr_locked.get_or_create_tls_acceptor(&domains).await {
                Ok(new_acceptor) => {
                    acceptor.reload(new_acceptor).await;
                    info!(
                        "Reloaded TLS acceptor with existing certificate for {}",
                        assigned_domain
                    );
                    *tlsforward_state.write().await = Some(new_state);
                }
                Err(e) => {
                    error!(
                        "Failed to reload TLS acceptor with existing certificate: {}",
                        e
                    );
                    *tlsforward_state.write().await = Some(TlsForwardState::Error(format!(
                        "Failed to reload existing certificate: {e}"
                    )));
                }
            }
        }
    }
}

/// Active daemon runtime components
struct DaemonRuntime {
    app_state: AppState<gate_daemon::ServerState>,
    endpoint: Arc<gate_p2p::Endpoint>,
    #[allow(dead_code)]
    tlsforward_service: Option<Arc<TlsForwardService>>,
    #[allow(dead_code)]
    tls_acceptor: Option<Arc<ReloadableTlsAcceptor>>,
    #[allow(dead_code)]
    p2p_router: Option<Router>,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

pub struct DaemonState {
    server_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    config: Arc<RwLock<Settings>>,
    runtime: Arc<RwLock<Option<DaemonRuntime>>>,
    tlsforward_state: Arc<RwLock<Option<TlsForwardState>>>,
    // Keep the state receiver to prevent the channel from being dropped
    tlsforward_state_rx:
        Arc<RwLock<Option<watch::Receiver<gate_daemon::services::tlsforward::TlsForwardState>>>>,
    state_dir: StateDir,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum TlsForwardState {
    Disabled,
    Disconnected,
    Connecting,
    Connected {
        server_address: String,
        assigned_domain: String,
    },
    Error(String),
}

impl DaemonState {
    pub fn new() -> Self {
        let state_dir = StateDir::new();

        // Try to load existing config
        let config = Self::load_config(&state_dir).unwrap_or_else(|_| Settings::default());

        Self {
            server_handle: Arc::new(RwLock::new(None)),
            config: Arc::new(RwLock::new(config)),
            runtime: Arc::new(RwLock::new(None)),
            tlsforward_state: Arc::new(RwLock::new(None)),
            tlsforward_state_rx: Arc::new(RwLock::new(None)),
            state_dir,
        }
    }

    fn config_path(state_dir: &StateDir) -> PathBuf {
        state_dir.config_dir().join("gui-config.json")
    }

    fn load_config(state_dir: &StateDir) -> Result<Settings> {
        let path = Self::config_path(state_dir);
        if path.exists() {
            let contents = std::fs::read_to_string(&path)?;
            let config: Settings = serde_json::from_str(&contents)?;
            debug!("Loaded GUI config from {}", path.display());
            Ok(config)
        } else {
            debug!("No existing GUI config found at {}", path.display());
            Err(anyhow::anyhow!("Config file not found"))
        }
    }

    async fn save_config(&self) -> Result<()> {
        let config = self.config.read().await.clone();
        let path = Self::config_path(&self.state_dir);

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let contents = serde_json::to_string_pretty(&config)?;
        tokio::fs::write(&path, contents).await?;
        debug!("Saved GUI config to {}", path.display());
        Ok(())
    }
}

#[tauri::command]
pub async fn start_daemon(
    state: State<'_, DaemonState>,
    app: AppHandle,
    config: Option<Settings>,
) -> Result<String, String> {
    let mut handle_guard = state.server_handle.write().await;

    if handle_guard.is_some() {
        return Err("Daemon is already running".to_string());
    }

    // Use provided config or the already loaded config
    let mut daemon_config = if let Some(cfg) = config {
        // Update stored config with new config
        *state.config.write().await = cfg.clone();
        cfg
    } else {
        // Use the existing loaded config
        state.config.read().await.clone()
    };

    // Ensure local inference is always enabled for GUI
    if daemon_config.local_inference.is_none() {
        info!("Enabling local inference for GUI daemon");
        daemon_config.local_inference = Some(gate_daemon::config::LocalInferenceConfig {
            enabled: true,
            max_concurrent_inferences: 1,
            default_temperature: 0.7,
            default_max_tokens: 1024,
        });

        // Update the stored config with local inference
        state.config.write().await.local_inference = daemon_config.local_inference.clone();
    }

    // Save config to disk
    if let Err(e) = state.save_config().await {
        error!("Failed to save GUI config: {}", e);
    }

    // Create shutdown channel
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // Create channels to pass runtime back
    let (runtime_tx, runtime_rx) = tokio::sync::oneshot::channel();
    let (tlsforward_state_tx, tlsforward_state_rx) = tokio::sync::oneshot::channel();

    // Spawn the daemon server
    let handle = tokio::spawn({
        let tlsforward_state = state.tlsforward_state.clone();
        let tlsforward_state_rx = state.tlsforward_state_rx.clone();

        async move {
            match run_daemon_server(
                daemon_config,
                DaemonRuntimeParams {
                    shutdown_rx,
                    shutdown_tx,
                    runtime_tx,
                    tlsforward_state_tx,
                    tlsforward_state,
                    tlsforward_state_rx,
                },
                app,
            )
            .await
            {
                Ok(_) => info!("Daemon server stopped cleanly"),
                Err(e) => error!("Daemon server error: {}", e),
            }
        }
    });

    *handle_guard = Some(handle);

    // Wait for runtime to be sent back
    match tokio::time::timeout(tokio::time::Duration::from_secs(5), runtime_rx).await {
        Ok(Ok(runtime)) => {
            *state.runtime.write().await = Some(runtime);
        }
        _ => {
            return Err("Failed to initialize daemon runtime".to_string());
        }
    }

    // Wait for TLS forward state to be sent back (if any)
    match tokio::time::timeout(tokio::time::Duration::from_secs(5), tlsforward_state_rx).await {
        Ok(Ok(tlsforward_state)) => {
            *state.tlsforward_state.write().await = tlsforward_state;
        }
        _ => {
            // TLS forward is optional
            info!("No TLS forward state initialized");
        }
    }

    Ok("Daemon started successfully".to_string())
}

#[tauri::command]
pub async fn stop_daemon(state: State<'_, DaemonState>) -> Result<String, String> {
    let mut handle_guard = state.server_handle.write().await;

    if let Some(handle) = handle_guard.take() {
        // Send shutdown signal via runtime
        if let Some(runtime) = state.runtime.write().await.take() {
            let _ = runtime.shutdown_tx.send(());
        }

        // Wait for the server to shutdown
        match tokio::time::timeout(tokio::time::Duration::from_secs(5), handle).await {
            Ok(_) => Ok("Daemon stopped successfully".to_string()),
            Err(_) => {
                error!("Daemon did not shutdown gracefully within timeout");
                Err("Daemon shutdown timeout".to_string())
            }
        }
    } else {
        Err("Daemon is not running".to_string())
    }
}

#[tauri::command]
pub async fn daemon_status(state: State<'_, DaemonState>) -> Result<bool, String> {
    let handle_guard = state.server_handle.read().await;
    Ok(handle_guard.is_some())
}

#[tauri::command]
pub async fn get_daemon_config(state: State<'_, DaemonState>) -> Result<Settings, String> {
    Ok(state.config.read().await.clone())
}

#[tauri::command]
pub async fn restart_daemon(
    state: State<'_, DaemonState>,
    app: AppHandle,
    config: Option<Settings>,
) -> Result<String, String> {
    // Stop if running
    let _ = stop_daemon(state.clone()).await;

    // Wait a bit for cleanup
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Start with new config
    start_daemon(state, app, config).await
}

#[allow(clippy::too_many_arguments)]
async fn run_daemon_server(
    config: Settings,
    params: DaemonRuntimeParams,
    app: AppHandle,
) -> Result<()> {
    // Destructure params
    let DaemonRuntimeParams {
        shutdown_rx,
        shutdown_tx,
        runtime_tx,
        tlsforward_state_tx,
        tlsforward_state,
        tlsforward_state_rx,
    } = params;

    info!(
        "Starting embedded daemon server on {}:{}",
        config.server.host, config.server.port
    );

    // Use the provided settings
    let mut settings = config;

    // Initialize state directory (for config files if needed)
    let state_dir = StateDir::new();
    state_dir.create_directories().await?;

    // Use SQLite database
    let database_url = format!(
        "sqlite://{}",
        state_dir.data_dir().join("gate.db").display()
    );

    // Enable WebAuthn for private.hellas.ai domain
    settings.auth.webauthn.enabled = true;
    settings.auth.webauthn.rp_id = "private.hellas.ai".to_string();
    settings.auth.webauthn.rp_name = "Gate Private Node".to_string();
    settings.auth.webauthn.rp_origin = "https://private.hellas.ai".to_string();
    settings.auth.webauthn.allow_subdomains = true;
    settings.auth.webauthn.allow_tlsforward_origins = true;

    // Create database backend
    let state_backend = Arc::new(SqliteStateBackend::new(&database_url).await?);
    let webauthn_backend = Arc::new(SqlxWebAuthnBackend::new(state_backend.pool().clone()));

    // Build server components
    let settings_arc = Arc::new(settings.clone());
    let builder = ServerBuilder::new(
        settings.clone(),
        state_backend.clone(),
        webauthn_backend,
        settings_arc.clone(),
    );

    let jwt_service = builder.build_jwt_service();
    let upstream_registry = builder.build_upstream_registry().await?;
    let app_state = builder
        .build_app_state(jwt_service, upstream_registry)
        .await?;

    // Check if bootstrap is needed and generate token if so
    {
        let bootstrap_manager = &app_state.data.bootstrap_manager;
        match bootstrap_manager.needs_bootstrap().await {
            Ok(true) => {
                info!("System needs bootstrap - generating token...");
                match bootstrap_manager.generate_token().await {
                    Ok(token) => {
                        info!("========================================");
                        info!("BOOTSTRAP TOKEN: {}", token);
                        info!("Use this token to create the admin user");
                        info!("This token is single-use only");
                        info!("========================================");
                    }
                    Err(e) => {
                        error!("Failed to generate bootstrap token: {}", e);
                    }
                }
            }
            Ok(false) => {
                debug!("System already bootstrapped");
            }
            Err(e) => {
                error!("Failed to check bootstrap status: {}", e);
            }
        }
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

    // Resolve static directory for frontend-daemon
    // In GUI crate, we're ALWAYS in a Tauri app - either dev mode or built
    let static_dir = if tauri::is_dev() {
        // Development mode - use the source directory
        let dev_path = "crates/frontend-daemon/dist";
        info!(
            "Running in Tauri dev mode, using development path: {}",
            dev_path
        );
        dev_path.to_string()
    } else {
        // Production mode - use Tauri's resource resolver
        use tauri::path::BaseDirectory;
        let path_resolver = app.path();

        match path_resolver.resolve("frontend-daemon", BaseDirectory::Resource) {
            Ok(path) => {
                info!(
                    "Running in production mode, resolved resources at: {}",
                    path.display()
                );

                // Log contents for debugging
                if let Ok(entries) = std::fs::read_dir(&path) {
                    debug!("Resource directory contents:");
                    for entry in entries.flatten() {
                        debug!("  - {}", entry.file_name().to_string_lossy());
                    }
                }

                path.to_string_lossy().to_string()
            }
            Err(e) => {
                error!("Failed to resolve frontend-daemon resources: {}", e);
                return Err(anyhow::anyhow!("Frontend resources not found in bundle"));
            }
        }
    };

    info!("Using static directory: {}", static_dir);
    let app = ServerBuilder::build_axum_router(router, app_state.clone(), Some(static_dir.clone()));

    // Apply correlation ID middleware
    let app = app.layer(axum::middleware::from_fn(
        gate_http::middleware::correlation_id_middleware,
    ));

    // Create P2P endpoint with persistent key
    let secret_key_path = state_dir.iroh_secret_key_path();
    let secret_key = helpers::load_or_create_p2p_secret_key(&secret_key_path).await?;

    // Create endpoint with optional static discovery
    let tlsforward_servers = if settings.tlsforward.enabled {
        settings.tlsforward.tlsforward_addresses.clone()
    } else {
        vec![]
    };

    let endpoint = helpers::create_p2p_endpoint(
        secret_key,
        true, // enable_discovery
        &tlsforward_servers,
    )
    .await?;

    // Initialize TLS forwarding if enabled
    let (tlsforward_service_opt, tls_acceptor_opt, p2p_router_opt) = if settings.tlsforward.enabled
    {
        info!("TLS forwarding is enabled");

        // Setup certificate manager and TLS acceptor
        let (tls_acceptor_inner, cert_manager) =
            helpers::setup_certificate_manager(&state_dir, settings.letsencrypt.enabled).await?;

        // Create HTTP server for TLS forward handler with same static file serving
        // Build router the same way as in main.rs
        let mut api_router = gate_http::routes::router();

        // Add all route modules
        api_router = gate_http::routes::dashboard::add_routes(api_router);
        api_router = gate_http::routes::inference::add_routes(api_router);
        api_router = gate_http::routes::models::add_routes(api_router);
        api_router = gate_http::routes::observability::add_routes(api_router);

        // Add daemon-specific routes
        api_router = gate_daemon::routes::config::add_routes(api_router);
        api_router = gate_daemon::routes::auth::add_routes(api_router);
        api_router = gate_daemon::routes::admin::add_routes(api_router);

        let axum_router = ServerBuilder::build_axum_router(
            api_router,
            app_state.clone(),
            Some(static_dir.clone()),
        );

        // Apply correlation ID middleware
        let axum_router = axum_router.layer(axum::middleware::from_fn(
            gate_http::middleware::correlation_id_middleware,
        ));

        let http_server = Arc::new(HttpServer::new(axum_router));

        let mut p2p_router = None;

        // Register TLS forward handler with P2P router if we have an acceptor
        if let Some(acceptor) = tls_acceptor_inner.as_ref() {
            // Create TLS forward handler to accept incoming connections
            let tls_handler = TlsForwardHandler::new(
                acceptor.as_ref().clone(),
                http_server.clone(),
                100, // max_connections
                30,  // connection_timeout_secs
            );

            // Create router and register the TLS forward handler
            let router = Router::builder(endpoint.as_ref().clone())
                .accept(TLS_FORWARD_ALPN, tls_handler)
                .spawn();

            info!("Registered TLS forward handler on P2P endpoint");
            p2p_router = Some(router);

            match start_tlsforward_service(settings.tlsforward.clone(), endpoint.clone()).await {
                Ok(service) => {
                    // Subscribe to state changes
                    let state_rx = service.subscribe();
                    let _ = tlsforward_state_tx
                        .send(Some(convert_tlsforward_state(&state_rx.borrow())));

                    // Store the state receiver for monitoring
                    *tlsforward_state_rx.write().await = Some(state_rx.clone());

                    // Setup certificate manager with TLS forward client
                    if let Some(cert_mgr) = cert_manager.as_ref() {
                        setup_cert_manager_client(&service, cert_mgr, &endpoint).await;
                    }

                    // Setup monitoring task for certificate requests
                    let state_rx = helpers::spawn_tlsforward_monitor(
                        service.clone(),
                        cert_manager.clone(),
                        tls_acceptor_inner.clone(),
                        settings.letsencrypt.email.clone(),
                        endpoint.clone(),
                        tlsforward_state.clone(),
                    )
                    .await;

                    // Store the state receiver for monitoring
                    *tlsforward_state_rx.write().await = Some(state_rx);

                    (Some(service), tls_acceptor_inner, p2p_router)
                }
                Err(e) => {
                    error!("Failed to start TLS forward service: {}", e);
                    let _ = tlsforward_state_tx.send(Some(TlsForwardState::Error(format!(
                        "Failed to start TLS forward service: {e}"
                    ))));
                    (None, tls_acceptor_inner, p2p_router)
                }
            }
        } else {
            // No TLS acceptor, just return p2p router
            (None, None, p2p_router)
        }
    } else {
        let _ = tlsforward_state_tx.send(None);
        (None, None, None)
    };

    // Create runtime struct and send it back
    let runtime = DaemonRuntime {
        app_state,
        endpoint,
        tlsforward_service: tlsforward_service_opt,
        tls_acceptor: tls_acceptor_opt,
        p2p_router: p2p_router_opt,
        shutdown_tx,
    };

    // Send runtime back to main thread
    let _ = runtime_tx.send(runtime);

    // Create TCP listener
    let addr = format!("{}:{}", settings.server.host, settings.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Daemon server listening on {}", addr);

    // Run server with graceful shutdown
    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = shutdown_rx.await;
        info!("Received shutdown signal");
    });

    server.await?;
    info!("Daemon server stopped");

    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DaemonRuntimeStatus {
    pub running: bool,
    pub listen_address: Option<String>,
    pub has_upstreams: bool,
}

#[tauri::command]
pub async fn get_daemon_status(
    state: State<'_, DaemonState>,
) -> Result<DaemonRuntimeStatus, String> {
    let handle_guard = state.server_handle.read().await;
    let is_running = handle_guard.is_some();

    let mut status = DaemonRuntimeStatus {
        running: is_running,
        listen_address: None,
        has_upstreams: false,
    };

    if is_running {
        let config = state.config.read().await;
        status.listen_address = Some(format!("{}:{}", config.server.host, config.server.port));

        // Check if we have upstreams configured
        if let Some(runtime) = &*state.runtime.read().await {
            status.has_upstreams = runtime.app_state.upstream_registry.has_upstreams().await;
        }
    }

    Ok(status)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DaemonRuntimeConfig {
    pub listen_address: String,
    pub database_url: String,
    pub upstream_count: usize,
    pub auth_enabled: bool,
    pub webauthn_enabled: bool,
    pub p2p_node_id: Option<String>,
    pub p2p_listen_addresses: Vec<String>,
    pub tlsforward_enabled: bool,
    pub tlsforward_state: Option<TlsForwardState>,
    pub needs_bootstrap: bool,
}

#[tauri::command]
pub async fn get_daemon_runtime_config(
    state: State<'_, DaemonState>,
) -> Result<DaemonRuntimeConfig, String> {
    let handle_guard = state.server_handle.read().await;
    if handle_guard.is_none() {
        return Err("Daemon is not running".to_string());
    }

    let config = state.config.read().await;
    let mut runtime_config = DaemonRuntimeConfig {
        listen_address: format!("{}:{}", config.server.host, config.server.port),
        database_url: ":memory:".to_string(), // GUI always uses in-memory SQLite
        upstream_count: 0,
        auth_enabled: false,
        webauthn_enabled: false,
        p2p_node_id: None,
        p2p_listen_addresses: Vec::new(),
        tlsforward_enabled: config.tlsforward.enabled,
        tlsforward_state: None,
        needs_bootstrap: false,
    };

    // Get detailed info from runtime
    if let Some(runtime) = &*state.runtime.read().await {
        // Count upstreams from settings
        runtime_config.upstream_count = runtime.app_state.data.settings.upstreams.len();

        // Get auth settings from server state
        runtime_config.auth_enabled = true; // Always true for now
        runtime_config.webauthn_enabled = runtime.app_state.data.settings.auth.webauthn.enabled;

        // Get P2P node ID
        runtime_config.p2p_node_id = Some(runtime.endpoint.node_id().to_string());

        // Get P2P listen addresses
        let socket_addrs = runtime.endpoint.bound_sockets();
        runtime_config.p2p_listen_addresses =
            socket_addrs.iter().map(|addr| addr.to_string()).collect();

        // Check if bootstrap is needed
        runtime_config.needs_bootstrap = runtime
            .app_state
            .data
            .bootstrap_manager
            .needs_bootstrap()
            .await
            .unwrap_or(false);
    }

    // Get TLS forward state
    runtime_config.tlsforward_state = state.tlsforward_state.read().await.clone();

    Ok(runtime_config)
}

#[tauri::command]
pub async fn get_tlsforward_status(
    state: State<'_, DaemonState>,
) -> Result<Option<TlsForwardState>, String> {
    Ok(state.tlsforward_state.read().await.clone())
}

#[tauri::command]
pub async fn configure_tlsforward(
    state: State<'_, DaemonState>,
    email: String,
) -> Result<String, String> {
    // Validate email format
    if !email.contains('@') || email.len() < 3 {
        return Err("Invalid email address".to_string());
    }

    // Update configuration
    let mut config = state.config.write().await;
    config.letsencrypt.email = Some(email);
    config.tlsforward.enabled = true;
    drop(config); // Release the lock before saving

    // Save config to disk
    if let Err(e) = state.save_config().await {
        error!("Failed to save GUI config: {}", e);
    }

    Ok("TLS forward configured successfully".to_string())
}

#[tauri::command]
pub async fn enable_tlsforward(
    state: State<'_, DaemonState>,
    app: AppHandle,
) -> Result<String, String> {
    // Check if email is configured
    let config = state.config.read().await;
    if config.letsencrypt.email.is_none() {
        return Err("Please configure email address first".to_string());
    }

    // Restart daemon with TLS forward enabled
    let new_config = config.clone();
    drop(config);

    restart_daemon(state, app, Some(new_config)).await
}

#[tauri::command]
pub async fn disable_tlsforward(
    state: State<'_, DaemonState>,
    app: AppHandle,
) -> Result<String, String> {
    let mut config = state.config.write().await;
    config.tlsforward.enabled = false;
    let new_config = config.clone();
    drop(config);

    // Update state immediately
    *state.tlsforward_state.write().await = Some(TlsForwardState::Disabled);

    // Restart daemon with TLS forward disabled
    restart_daemon(state, app, Some(new_config)).await
}

/// Start TLS forward service
async fn start_tlsforward_service(
    config: gate_daemon::config::TlsForwardConfig,
    endpoint: Arc<gate_p2p::Endpoint>,
) -> Result<Arc<TlsForwardService>> {
    // Build TLS forward service with the existing endpoint
    TlsForwardService::builder(config, endpoint).build().await
}

/// Setup certificate manager with TLS forward client
async fn setup_cert_manager_client(
    service: &Arc<TlsForwardService>,
    cert_mgr: &Arc<Mutex<CertificateManager>>,
    endpoint: &Arc<gate_p2p::Endpoint>,
) {
    // Get the actual connected TLS forward server address from the service
    if let Some(tlsforward_node_id) = service.tlsforward_node_id().await {
        info!(
            "Setting up certificate manager with TLS forward server: {}",
            tlsforward_node_id
        );

        let tls_forward_client =
            gate_tlsforward::TlsForwardClient::new(endpoint.clone(), tlsforward_node_id);
        cert_mgr
            .lock()
            .await
            .set_tls_forward_client(tls_forward_client);
    } else {
        warn!("TLS forward service started but no node ID available yet");
    }
}

/// Bootstrap URL information
#[derive(Clone, Serialize, Deserialize)]
pub struct BootstrapUrlInfo {
    pub url: String,
    pub needs_bootstrap: bool,
}

#[tauri::command]
pub async fn get_bootstrap_url(state: State<'_, DaemonState>) -> Result<BootstrapUrlInfo, String> {
    // Check if daemon is running
    let handle_guard = state.server_handle.read().await;
    if handle_guard.is_none() {
        return Err("Daemon is not running".to_string());
    }

    // Get the daemon configuration to construct the URL
    let config = state.config.read().await;
    let base_url = format!("http://{}:{}", config.server.host, config.server.port);

    // Check if bootstrap is needed
    let needs_bootstrap = if let Some(runtime) = &*state.runtime.read().await {
        // Check if bootstrap is needed by checking if there are any users
        runtime
            .app_state
            .data
            .bootstrap_manager
            .needs_bootstrap()
            .await
            .unwrap_or(false)
    } else {
        false
    };

    Ok(BootstrapUrlInfo {
        url: base_url,
        needs_bootstrap,
    })
}

/// Convert TLS forward state from daemon to GUI format
fn convert_tlsforward_state(
    state: &gate_daemon::services::tlsforward::TlsForwardState,
) -> TlsForwardState {
    match state {
        gate_daemon::services::tlsforward::TlsForwardState::Disconnected => {
            TlsForwardState::Disconnected
        }
        gate_daemon::services::tlsforward::TlsForwardState::Connecting => {
            TlsForwardState::Connecting
        }
        gate_daemon::services::tlsforward::TlsForwardState::Connected {
            tlsforward_node,
            assigned_domain,
        } => TlsForwardState::Connected {
            server_address: format!("{tlsforward_node}"),
            assigned_domain: assigned_domain.clone(),
        },
        gate_daemon::services::tlsforward::TlsForwardState::Error(msg) => {
            TlsForwardState::Error(msg.clone())
        }
    }
}

/// Get bootstrap token for initial admin setup
#[tauri::command]
pub async fn get_bootstrap_token(state: State<'_, DaemonState>) -> Result<Option<String>, String> {
    // Check if daemon is running
    let handle_guard = state.server_handle.read().await;
    if handle_guard.is_none() {
        return Err("Daemon is not running".to_string());
    }

    // Get bootstrap token from runtime
    if let Some(runtime) = &*state.runtime.read().await {
        let bootstrap_manager = &runtime.app_state.data.bootstrap_manager;

        // Check if bootstrap is needed
        let needs_bootstrap = bootstrap_manager
            .needs_bootstrap()
            .await
            .map_err(|e| format!("Failed to check bootstrap status: {e}"))?;

        if !needs_bootstrap {
            // System is already bootstrapped
            return Ok(None);
        }

        // Check if bootstrap is already complete (token used)
        if bootstrap_manager.is_bootstrap_complete().await {
            return Ok(None);
        }

        // Get or generate token
        let token = bootstrap_manager
            .generate_token()
            .await
            .map_err(|e| format!("Failed to generate bootstrap token: {e}"))?;

        // Log the token to console for visibility
        info!("========================================");
        info!("BOOTSTRAP TOKEN: {}", token);
        info!("Use this token to create the admin user");
        info!("This token is single-use only");
        info!("========================================");

        Ok(Some(token))
    } else {
        Err("Daemon runtime not initialized".to_string())
    }
}
