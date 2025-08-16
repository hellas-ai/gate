//! Gate TLS forward server binary
#[macro_use]
extern crate tracing;

use anyhow::Result;
use axum::{Router as AxumRouter, routing::get};
use clap::Parser;
use futures::stream::StreamExt;
use gate_core::tracing::{
    config::{InstrumentationConfig, OtlpConfig},
    init::init_tracing,
    metrics::{self, gauge},
    prometheus::export_prometheus,
};
use gate_tlsforward::server::{
    config::TlsForwardConfig,
    dns_challenge::DnsChallengeManager,
    http_handler::TlsForwardHttpHandler,
    https_proxy::{HttpsProxy, HttpsProxyConfig},
    registry::ProxyRegistry,
    state_dir::TlsForwardStateDir,
};
use iroh::{Endpoint, Watcher, protocol::Router};
use std::sync::Arc;

/// Gate TLS forward server - P2P TLS forwarding for secure HTTPS tunneling
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Configuration file path
    #[arg(short = 'c', long = "config")]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let cli = Cli::parse();

    // Load initial configuration
    let config = if let Some(config_path) = &cli.config {
        info!("Loading configuration from: {}", config_path);
        TlsForwardConfig::load_from_file(config_path)?
    } else {
        TlsForwardConfig::load()?
    };

    // Initialize instrumentation
    let instrumentation_config = InstrumentationConfig {
        service_name: "gate-tlsforward".to_string(),
        service_version: env!("CARGO_PKG_VERSION").to_string(),
        log_level: std::env::var("RUST_LOG").unwrap_or_else(|_| config.server.log_level.clone()),
        otlp: std::env::var("OTLP_ENDPOINT")
            .ok()
            .map(|endpoint| OtlpConfig {
                endpoint,
                headers: None,
            }),
    };
    init_tracing(&instrumentation_config)?;

    info!(
        "Starting Gate TLS Forward Server with configuration: {:#?}",
        config
    );

    // Create state directory manager
    let state_dir = TlsForwardStateDir::new();
    state_dir.create_directories().await?;

    // Load or generate secret key
    let secret_key_path = config
        .p2p
        .secret_key_path
        .clone()
        .or_else(|| Some(state_dir.secret_key_path().to_string_lossy().into_owned()));

    let secret_key = if let Some(path) = &secret_key_path {
        match tokio::fs::read_to_string(path).await {
            Ok(contents) => {
                let hex_key = contents.trim();
                match hex::decode(hex_key) {
                    Ok(bytes) if bytes.len() == 32 => {
                        let mut key_bytes = [0u8; 32];
                        key_bytes.copy_from_slice(&bytes);
                        info!("Loaded secret key from {}", path);
                        Some(iroh::SecretKey::from_bytes(&key_bytes))
                    }
                    _ => {
                        error!("Invalid secret key format in {}, generating new key", path);
                        None
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info!("Secret key file not found at {}, will create new key", path);
                None
            }
            Err(e) => {
                error!("Failed to read secret key from {}: {}", path, e);
                return Err(e.into());
            }
        }
    } else {
        None
    };

    // Create P2P endpoint
    let mut builder = Endpoint::builder();

    if let Some(secret_key) = secret_key.clone() {
        builder = builder.secret_key(secret_key);
    }

    for bind_addr in config.p2p.bind_addrs.iter() {
        info!("Setting P2P bind address: {}", bind_addr);
        match bind_addr {
            std::net::SocketAddr::V4(addr) => {
                builder = builder.bind_addr_v4(*addr);
            }
            std::net::SocketAddr::V6(addr) => {
                builder = builder.bind_addr_v6(*addr);
            }
        }
    }

    if config.p2p.enable_discovery {
        builder = builder.discovery_n0();
    }

    // Add ALPN protocols
    builder = builder.alpns(vec![gate_tlsforward::TLSFORWARD_HTTP_ALPN.to_vec()]);

    let endpoint = builder.bind().await?;
    let node_id = endpoint.node_id();

    // Save the secret key if we have a path and didn't load one
    if let (Some(path), None) = (&secret_key_path, &secret_key) {
        let key_bytes = endpoint.secret_key().to_bytes();
        let hex_key = hex::encode(key_bytes);

        // Create parent directory if it doesn't exist
        if let Some(parent) = std::path::Path::new(path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(path, hex_key).await?;
        info!("Saved new secret key to {}", path);
    }

    info!("P2P endpoint started, waiting to get node address...");
    let node_addrs = loop {
        if let Some(node_addr) = endpoint.node_addr().stream().next().await {
            if let Some(addr) = node_addr {
                info!("Node address obtained: {:?}", addr);
                break addr;
            } else {
                info!("Waiting for node address...");
            }
        }
    };

    info!("Node ID: {}", node_id);
    info!("Node address: {:?}", node_addrs);

    // Initialize tlsforward metrics
    gauge("tlsforward_info").set(1);
    gauge("tlsforward_https_connections_active").set(0);
    gauge("tlsforward_http_requests_active").set(0);
    gauge("tlsforward_registry_nodes").set(0);

    // Create registry
    let registry = ProxyRegistry::new(config.https_proxy.domain_suffix.clone());
    let registry_arc = Arc::new(registry.clone());

    // Create DNS challenge manager for ACME if Cloudflare is configured
    let dns_challenge_manager = if let (Some(api_token), Some(zone_id)) = (
        &config.dns.cloudflare.api_token,
        &config.dns.cloudflare.zone_id,
    ) {
        match DnsChallengeManager::new(
            api_token.clone(),
            zone_id.clone(),
            config.https_proxy.domain_suffix.clone(),
        ) {
            Ok(manager) => Arc::new(manager),
            Err(e) => {
                error!("Failed to create DNS challenge manager: {}", e);
                return Err(e);
            }
        }
    } else {
        error!("DNS challenge manager requires Cloudflare configuration");
        return Err(anyhow::anyhow!(
            "Missing Cloudflare configuration for DNS challenges"
        ));
    };

    // Build the router with protocol handlers
    let router = Router::builder(endpoint.clone())
        // HTTP handler (serves REST APIs for ACME and tlsforward control)
        .accept(
            gate_tlsforward::TLSFORWARD_HTTP_ALPN,
            TlsForwardHttpHandler::new(
                dns_challenge_manager,
                registry_arc.clone(),
                config.https_proxy.domain_suffix.clone(),
                node_id,
                // node_addrs.clone(),
            ),
        )
        .spawn();

    info!("P2P router started and accepting connections");

    // Create and start HTTPS proxy
    let https_config = HttpsProxyConfig {
        bind_addr: config.https_proxy.bind_addr,
        timeouts: Default::default(),
        max_connections: config.https_proxy.max_connections,
        buffer_size: 16 * 1024,
    };

    let https_proxy = Arc::new(HttpsProxy::new(https_config, registry, endpoint.clone()));

    // Start HTTPS proxy in background
    let proxy_handle = tokio::spawn(async move {
        if let Err(e) = https_proxy.start().await {
            error!("HTTPS proxy error: {}", e);
        }
    });

    info!("HTTPS proxy listening on {}", config.https_proxy.bind_addr);

    // Start metrics server if configured
    let metrics_handle = if let Some(metrics_addr) = config.server.metrics_addr {
        info!("Starting metrics server on {}", metrics_addr);

        let metrics_router = AxumRouter::new().route(
            "/metrics",
            get(|| async { export_prometheus(metrics::global()) }),
        );

        let listener = tokio::net::TcpListener::bind(metrics_addr).await?;
        info!(
            "Prometheus metrics endpoint available at http://{}/metrics",
            metrics_addr
        );

        Some(tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, metrics_router).await {
                error!("Metrics server error: {}", e);
            }
        }))
    } else {
        info!("Metrics server not configured (set server.metrics_addr to enable)");
        None
    };

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    // Graceful shutdown
    proxy_handle.abort();
    if let Some(handle) = metrics_handle {
        handle.abort();
    }
    router.shutdown().await?;

    Ok(())
}
