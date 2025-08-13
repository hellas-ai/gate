use anyhow::Result;
use gate_http::AppState;
use gate_p2p::{Endpoint, NodeAddr, SecretKey, discovery::static_provider::StaticProvider};
use gate_tlsforward::{CertificateManager, TLS_FORWARD_ALPN, TlsForwardClient, TlsForwardHandler};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::{services::TlsForwardService, tls_reload::ReloadableTlsAcceptor};

/// Load or create P2P secret key
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
    data_dir: &Path,
    initial_domains: Vec<String>,
) -> Result<(Arc<ReloadableTlsAcceptor>, Arc<Mutex<CertificateManager>>)> {
    // Create certificate manager
    let cert_manager = Arc::new(Mutex::new(CertificateManager::new(data_dir.to_path_buf())));

    // Get domains for certificate
    let mut domains = initial_domains;

    // Only add localhost if no other domains are configured
    if domains.is_empty() {
        domains.push("localhost".to_string());
    }

    // Get or create TLS acceptor
    let acceptor = cert_manager
        .lock()
        .await
        .get_or_create_tls_acceptor(&domains)
        .await?;
    let reloadable_acceptor = Arc::new(ReloadableTlsAcceptor::new(acceptor));

    Ok((reloadable_acceptor, cert_manager))
}

/// Setup TLS forward monitoring task for WebAuthn updates
pub async fn spawn_webauthn_monitor(
    service: Arc<TlsForwardService>,
    webauthn_service: Arc<gate_http::services::WebAuthnService>,
) {
    let mut state_rx = service.subscribe();
    let mut last_domain: Option<String> = None;

    tokio::spawn(async move {
        while state_rx.changed().await.is_ok() {
            let state = state_rx.borrow().clone();
            if let crate::services::TlsForwardState::Connected {
                assigned_domain, ..
            } = &state
                && last_domain.as_ref() != Some(assigned_domain)
            {
                info!(
                    "TLS forward connected with domain: {}, updating WebAuthn allowed origins",
                    assigned_domain
                );

                let tlsforward_origin = format!("https://{assigned_domain}");

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
                    last_domain = Some(assigned_domain.clone());
                }
            }
        }
    });
}

/// Request Let's Encrypt certificates for configured domains
pub async fn request_letsencrypt_certificates(
    certificate_manager: Arc<Mutex<CertificateManager>>,
    reloadable_acceptor: Arc<ReloadableTlsAcceptor>,
    domains: Vec<String>,
    email: &str,
) -> Result<()> {
    if domains.is_empty() {
        return Ok(());
    }

    info!("Requesting certificates for configured domains");
    for domain in &domains {
        info!("Checking certificate for domain: https://{}", domain);

        let cert_mgr = certificate_manager.lock().await;
        if !cert_mgr.has_certificate(domain).await {
            info!("Requesting new certificate for https://{}", domain);
            match cert_mgr.request_certificate(domain, email).await {
                Ok(()) => {
                    info!("Successfully obtained certificate for {}", domain);
                    // Reload TLS acceptor with new certificates
                    if let Ok(new_acceptor) = cert_mgr.get_or_create_tls_acceptor(&domains).await {
                        reloadable_acceptor.reload(new_acceptor).await;
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

    Ok(())
}

/// Setup certificate manager with TLS forward client
pub async fn setup_cert_manager_client(
    service: &Arc<TlsForwardService>,
    cert_mgr: &Arc<Mutex<CertificateManager>>,
    endpoint: &Arc<Endpoint>,
) {
    // Get the actual connected TLS forward server address from the service
    if let Some(tlsforward_node_id) = service.tlsforward_node_id().await {
        info!(
            "Setting up certificate manager with TLS forward server: {}",
            tlsforward_node_id
        );

        let tls_forward_client = TlsForwardClient::new(endpoint.clone(), tlsforward_node_id);
        cert_mgr
            .lock()
            .await
            .set_tls_forward_client(tls_forward_client);
    } else {
        warn!("TLS forward service started but no node ID available yet");
    }
}

/// Build complete daemon router with all routes
pub fn build_daemon_router() -> utoipa_axum::router::OpenApiRouter<AppState<crate::ServerState>> {
    let mut router = gate_http::routes::router();

    // Add all standard route modules
    router = gate_http::routes::dashboard::add_routes(router);
    router = gate_http::routes::inference::add_routes(router);
    router = gate_http::routes::models::add_routes(router);
    router = gate_http::routes::observability::add_routes(router);

    // Add daemon-specific routes
    router = crate::routes::config::add_routes(router);
    router = crate::routes::auth::add_routes(router);
    router = crate::routes::admin::add_routes(router);

    router
}

/// Create P2P router with TLS forward handler
pub fn create_p2p_router(
    endpoint: Arc<Endpoint>,
    tls_acceptor: Arc<ReloadableTlsAcceptor>,
    http_server: Arc<gate_http::server::HttpServer>,
    max_connections: usize,
    connection_timeout_secs: u64,
) -> gate_p2p::Router {
    // Create TLS forward handler to accept incoming connections
    let tls_handler = TlsForwardHandler::new(
        tls_acceptor.as_ref().clone(),
        http_server,
        max_connections,
        connection_timeout_secs,
    );

    // Create router and register the TLS forward handler
    let router = gate_p2p::Router::builder(endpoint.as_ref().clone())
        .accept(TLS_FORWARD_ALPN, tls_handler)
        .spawn();

    debug!("Registered TLS forward handler on P2P endpoint");
    router
}

/// Start TLS forward service
pub async fn start_tlsforward_service(
    config: crate::config::TlsForwardConfig,
    endpoint: Arc<Endpoint>,
) -> Result<Arc<TlsForwardService>> {
    // Build TLS forward service with the existing endpoint
    TlsForwardService::builder(config, endpoint).build().await
}
