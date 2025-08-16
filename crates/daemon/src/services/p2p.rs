use anyhow::Result;
use gate_http::server::HttpServer;
use gate_p2p::{Endpoint, NodeAddr, Router, SecretKey, discovery::static_provider::StaticProvider};
use gate_tlsforward::{TLS_FORWARD_ALPN, TlsForwardHandler};
use std::path::Path;
use std::sync::Arc;

use crate::{
    config::TlsForwardConfig, services::TlsForwardService, tls_reload::ReloadableTlsAcceptor,
};

/// P2P configuration
pub struct P2PConfig {
    pub secret_key_path: std::path::PathBuf,
    pub enable_discovery: bool,
    pub tlsforward_addresses: Vec<String>,
}

/// Manages P2P endpoint and routing
#[derive(Clone)]
pub struct P2PManager {
    endpoint: Arc<Endpoint>,
    router: Option<Router>,
}

impl P2PManager {
    /// Create a new P2P manager
    pub async fn new(config: P2PConfig) -> Result<Self> {
        // Load or create P2P secret key
        let secret_key = load_or_create_p2p_secret_key(&config.secret_key_path).await?;

        // Create P2P endpoint
        let endpoint = create_p2p_endpoint(
            secret_key,
            config.enable_discovery,
            &config.tlsforward_addresses,
        )
        .await?;

        info!("P2P endpoint created with node ID: {}", endpoint.node_id());

        Ok(Self {
            endpoint,
            router: None,
        })
    }

    /// Get the P2P endpoint
    pub fn endpoint(&self) -> Arc<Endpoint> {
        self.endpoint.clone()
    }

    /// Setup TLS forward handler for accepting incoming connections
    pub fn setup_tls_forward_handler(
        &mut self,
        tls_acceptor: Arc<ReloadableTlsAcceptor>,
        http_server: Arc<HttpServer>,
        max_connections: usize,
        connection_timeout_secs: u64,
    ) -> Result<()> {
        // Create TLS forward handler
        let tls_handler = TlsForwardHandler::new(
            tls_acceptor.as_ref().clone(),
            http_server,
            max_connections,
            connection_timeout_secs,
        );

        // Create router and register the TLS forward handler
        let router = Router::builder(self.endpoint.as_ref().clone())
            .accept(TLS_FORWARD_ALPN, tls_handler)
            .spawn();

        debug!("Registered TLS forward handler on P2P endpoint");
        self.router = Some(router);

        Ok(())
    }

    /// Start TLS forward client service
    pub async fn start_tlsforward_service(
        &self,
        config: TlsForwardConfig,
    ) -> Result<Arc<TlsForwardService>> {
        debug!("Initializing TLS forward client service");

        let service = TlsForwardService::builder(config, self.endpoint.clone())
            .build()
            .await?;

        Ok(service)
    }

    /// Wait for TLS forward service to connect and return node ID
    pub async fn wait_for_tlsforward_connection(
        &self,
        service: &Arc<TlsForwardService>,
        timeout_secs: u64,
    ) -> Option<gate_p2p::NodeId> {
        let mut retry_count = 0;
        loop {
            if let Some(node_id) = service.tlsforward_node_id().await {
                return Some(node_id);
            }
            retry_count += 1;
            if retry_count > timeout_secs {
                warn!(
                    "TLS forward server not connected after {} seconds",
                    timeout_secs
                );
                return None;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
}

/// Load or create P2P secret key
async fn load_or_create_p2p_secret_key(path: &Path) -> Result<SecretKey> {
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
async fn create_p2p_endpoint(
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
