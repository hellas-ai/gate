//! TLS forward service for managing P2P TLS forwarding connections

use crate::config::TlsForwardConfig;
use crate::tracing::Instrument;
use anyhow::{Context, Result};
use gate_p2p::Endpoint;
use gate_tlsforward::TlsForwardClient;
use iroh::NodeId;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, watch};
use tokio::time;

/// TLS forward service state
#[derive(Debug, Clone, PartialEq)]
pub enum TlsForwardState {
    /// Not connected to any relay
    Disconnected,
    /// Connecting to TLS forward server
    Connecting,
    /// Connected and registered with relay
    Connected {
        tlsforward_node: NodeId,
        assigned_domain: String,
    },
    /// Connection error
    Error(String),
}

/// Internal state for active TLS forward connection
struct ActiveTlsForwardConnection {
    tls_forward_client: TlsForwardClient,
    node_id: NodeId,
}

/// Builder for configuring TlsForwardService
pub struct TlsForwardServiceBuilder {
    config: TlsForwardConfig,
    endpoint: Arc<Endpoint>,
}

impl TlsForwardServiceBuilder {
    /// Create a new builder with the given config and TLS handler
    pub fn new(config: TlsForwardConfig, endpoint: Arc<Endpoint>) -> Self {
        Self { config, endpoint }
    }

    /// Build and start the TLS forward service
    pub async fn build(self) -> Result<Arc<TlsForwardService>> {
        debug!("TlsForward config: {:?}", self.config);

        if !self.config.enabled {
            warn!(
                "Tried creating TlsForwardServiceBuilder but TLS forward service is disabled: {:?}",
                self.config
            );
            return Err(anyhow::anyhow!("TLS forward service is disabled"));
        }

        // Create channels
        let (state_tx, state_rx) = watch::channel(TlsForwardState::Disconnected);
        let (shutdown_tx, _) = watch::channel(false);

        let service = Arc::new(TlsForwardService {
            config: self.config,
            endpoint: self.endpoint.clone(),
            active_connection: Arc::new(RwLock::new(None)),
            state_tx,
            state_rx,
            shutdown_tx,
        });

        // Start connection loop
        let service_clone = service.clone();
        tokio::spawn(async move {
            service_clone.connection_loop().await;
        });

        Ok(service)
    }
}

/// TLS forward service for managing connections to TLS forward servers
pub struct TlsForwardService {
    config: TlsForwardConfig,
    endpoint: Arc<Endpoint>,
    active_connection: Arc<RwLock<Option<ActiveTlsForwardConnection>>>,
    state_tx: watch::Sender<TlsForwardState>,
    state_rx: watch::Receiver<TlsForwardState>,
    shutdown_tx: watch::Sender<bool>,
}

impl TlsForwardService {
    /// Create a new builder for the TLS forward service
    pub fn builder(config: TlsForwardConfig, endpoint: Arc<Endpoint>) -> TlsForwardServiceBuilder {
        TlsForwardServiceBuilder::new(config, endpoint)
    }

    /// Get current relay state
    pub fn state(&self) -> TlsForwardState {
        self.state_rx.borrow().clone()
    }

    /// Subscribe to state changes
    pub fn subscribe(&self) -> watch::Receiver<TlsForwardState> {
        self.state_rx.clone()
    }

    /// Get the endpoint reference
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Get the connected relay node ID
    pub async fn tlsforward_node_id(&self) -> Option<iroh::NodeId> {
        let connection = self.active_connection.read().await;
        connection.as_ref().map(|c| c.node_id)
    }

    /// Main connection loop
    async fn connection_loop(&self) {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let mut reconnect_attempts = 0;

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("TLS forward service shutting down");
                        break;
                    }
                }
                result = self.connect_to_relay(&mut reconnect_attempts) => {
                    if let Err(e) = result {
                        error!("Failed to connect to relay: {}", e);
                    }

                    if !self.config.auto_reconnect {
                        break;
                    }

                    // Wait before reconnecting
                    let backoff = Duration::from_secs(
                        self.config.reconnect_backoff * (reconnect_attempts as u64 + 1)
                    );
                    info!("Waiting {}s before reconnecting (attempt {})", backoff.as_secs(), reconnect_attempts + 1);
                    time::sleep(backoff).await;
                }
            }
        }
    }

    /// Connect to relay server
    async fn connect_to_relay(&self, reconnect_attempts: &mut u32) -> Result<()> {
        debug!("Attempting to connect to relay...");

        // Update state
        self.state_tx.send(TlsForwardState::Connecting)?;

        debug!("Endpoint ready, parsing relay addresses...");

        // Try each relay address
        for tlsforward_addr_str in &self.config.tlsforward_addresses {
            debug!("Trying TLS forward address: {}", tlsforward_addr_str);

            // Parse node ID from address string
            // We now only care about the node ID part, as addresses are handled by discovery
            let node_id =
                if let Some((node_id_str, _addr_str)) = tlsforward_addr_str.split_once('@') {
                    // Format: node_id@address:port - extract just the node ID
                    node_id_str
                        .parse::<iroh::NodeId>()
                        .map_err(|e| anyhow::anyhow!("Invalid node ID '{}': {}", node_id_str, e))?
                } else {
                    // Just a node ID
                    tlsforward_addr_str.parse::<iroh::NodeId>().map_err(|e| {
                        anyhow::anyhow!("Invalid node ID '{}': {}", tlsforward_addr_str, e)
                    })?
                };

            debug!("Connecting to TLS forward server with node ID: {}", node_id);

            match self.connect_to_tlsforward_addr(node_id).await {
                Ok(assigned_domain) => {
                    info!(
                        "Connected to tlsforward. Assigned domain: {}",
                        assigned_domain
                    );
                    self.state_tx.send(TlsForwardState::Connected {
                        tlsforward_node: node_id,
                        assigned_domain: assigned_domain.clone(),
                    })?;

                    *reconnect_attempts = 0;

                    // Start heartbeat loop
                    self.heartbeat_loop().await;

                    return Ok(());
                }
                Err(e) => {
                    error!("Failed to connect to TLS forward server {}: {}", node_id, e);
                    continue;
                }
            }
        }

        // All relays failed
        *reconnect_attempts += 1;
        let error_msg = "Failed to connect to any TLS forward server".to_string();
        self.state_tx
            .send(TlsForwardState::Error(error_msg.clone()))?;

        if *reconnect_attempts >= self.config.max_reconnect_attempts {
            error!("Maximum reconnection attempts reached");
            return Err(anyhow::anyhow!("Maximum reconnection attempts reached"));
        }

        Err(anyhow::anyhow!(error_msg))
    }

    /// Connect to a specific TLS forward address
    async fn connect_to_tlsforward_addr(&self, node_id: NodeId) -> Result<String> {
        let span = tracing::info_span!(
            "tlsforward.connect",
            tlsforward_node = %node_id,
        );

        async move {
            // Create TLS forward client
            let tls_forward_client = TlsForwardClient::new(self.endpoint.clone(), node_id);

            // Register with TLS forward server
            let (assigned_domain, _tlsforward_info) = tls_forward_client
                .register()
                .await
                .context("Failed to register with TLS forward server")?;

            // Record assigned domain in span
            tracing::Span::current().record("assigned_domain", &assigned_domain);

            // Store active connection
            let active = ActiveTlsForwardConnection {
                tls_forward_client,
                node_id,
            };
            *self.active_connection.write().await = Some(active);

            Ok(assigned_domain)
        }
        .instrument(span)
        .await
    }

    /// Heartbeat loop to maintain connection
    async fn heartbeat_loop(&self) {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let heartbeat_interval = Duration::from_secs(self.config.heartbeat_interval);
        let mut interval = time::interval(heartbeat_interval);

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        break;
                    }
                }
                _ = interval.tick() => {
                    if let Err(e) = self.send_heartbeat().await {
                        error!("Heartbeat failed: {}", e);
                        self.state_tx.send(TlsForwardState::Disconnected).ok();
                        break;
                    }
                }
            }
        }
    }

    /// Send heartbeat to TLS forward server
    async fn send_heartbeat(&self) -> Result<()> {
        let connection = self.active_connection.read().await;
        let active = connection
            .as_ref()
            .context("No active TLS forward connection")?;

        // Send ping with timeout
        tokio::time::timeout(Duration::from_secs(10), active.tls_forward_client.ping())
            .await
            .map_err(|_| anyhow::anyhow!("Heartbeat timeout"))?
            .context("Heartbeat ping failed")?;

        debug!("Heartbeat sent successfully");
        Ok(())
    }

    /// Disconnect from TLS forward server
    pub async fn disconnect(&self) -> Result<()> {
        info!("Disconnecting from TLS forward server");

        // Unregister from TLS forward server
        if let Some(active) = self.active_connection.write().await.take()
            && let Err(e) = active.tls_forward_client.unregister().await
        {
            warn!("Failed to unregister from TLS forward server: {}", e);
        }

        // Update state
        self.state_tx.send(TlsForwardState::Disconnected)?;

        Ok(())
    }

    /// Shutdown the service
    pub async fn shutdown(&self) {
        info!("Shutting down TLS forward service");
        self.shutdown_tx.send(true).ok();
        self.disconnect().await.ok();
    }
}

impl Drop for TlsForwardService {
    fn drop(&mut self) {
        // Trigger shutdown when service is dropped
        self.shutdown_tx.send(true).ok();
    }
}
