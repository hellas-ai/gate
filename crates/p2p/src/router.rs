//! Simplified P2P router using iroh's Router directly

use iroh::{
    Endpoint, SecretKey,
    protocol::{ProtocolHandler, Router, RouterBuilder},
};
use std::net::SocketAddr;
use thiserror::Error;

/// Configuration for P2P router
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// Secret key for the node (if not provided, one will be generated)
    pub secret_key: Option<SecretKey>,
    /// Bind address for the endpoint
    pub bind_addrs: Vec<SocketAddr>,
    /// Enable local network discovery
    pub enable_discovery: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            secret_key: None,
            bind_addrs: vec![
                // Default to dual-stack IPv4/IPv6 address
                SocketAddr::from(([0, 0, 0, 0], 31145)),
                SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], 31145)),
            ],
            enable_discovery: true,
        }
    }
}

#[derive(Debug, Error)]
pub enum RouterError {
    #[error("Endpoint bind error: {0}")]
    EndpointBind(#[from] Box<dyn std::error::Error + Send + Sync>),
}

type Result<T> = std::result::Result<T, RouterError>;

/// Create and configure a P2P router
pub async fn create_router(config: RouterConfig) -> Result<RouterBuilder> {
    // Create endpoint builder
    let mut builder = Endpoint::builder();

    // Set secret key if provided
    if let Some(secret_key) = config.secret_key {
        builder = builder.secret_key(secret_key);
    }

    // Set bind address if provided
    for bind_addr in config.bind_addrs {
        info!("P2P bind address configuration: {}", bind_addr);
        match bind_addr {
            SocketAddr::V4(addr) => builder = builder.bind_addr_v4(addr),
            SocketAddr::V6(addr) => builder = builder.bind_addr_v6(addr),
        }
    }

    // Configure discovery
    if config.enable_discovery {
        // In v0.90.0, use ConcurrentDiscovery for local network discovery
        let discovery = iroh::discovery::ConcurrentDiscovery::default();
        builder = builder.discovery(discovery);
    }

    // Build endpoint
    let endpoint = builder
        .bind()
        .await
        .map_err(|e| RouterError::EndpointBind(Box::new(e)))?;

    Ok(Router::builder(endpoint))
}

/// Helper function to create and start a router with multiple protocols
pub async fn start_router<I, T>(config: RouterConfig, protocols: I) -> Result<Router>
where
    I: IntoIterator<Item = (Vec<u8>, T)>,
    T: ProtocolHandler,
{
    let mut builder = create_router(config).await?;

    // Add all protocols
    for (alpn, handler) in protocols {
        builder = builder.accept(alpn, handler);
    }

    // Spawn the router
    Ok(builder.spawn())
}
