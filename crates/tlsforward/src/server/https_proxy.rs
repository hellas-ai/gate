//! HTTPS proxy server that forwards TLS traffic over P2P connections

use crate::TLS_FORWARD_ALPN;
use crate::common::error::Result;
use crate::server::config::ProxyTimeouts;
use crate::server::registry::ProxyRegistry;
use crate::server::sni::extract_sni;
use gate_core::tracing::{
    metrics::{counter, gauge, histogram},
    prelude::*,
};
use gate_p2p::stream::{CombinedStream, StreamUtils};
use iroh::Endpoint;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

/// HTTPS proxy server configuration
#[derive(Debug, Clone)]
pub struct HttpsProxyConfig {
    /// Address to bind the HTTPS proxy to
    pub bind_addr: SocketAddr,
    /// Timeouts for various operations
    pub timeouts: ProxyTimeouts,
    /// Maximum concurrent connections
    pub max_connections: usize,
    /// Buffer size for copying data
    pub buffer_size: usize,
}

impl Default for HttpsProxyConfig {
    fn default() -> Self {
        Self {
            // Use :: for dual-stack IPv4/IPv6 support
            bind_addr: "[::]:443"
                .parse()
                .unwrap_or_else(|_| ([0, 0, 0, 0], 443).into()),
            timeouts: ProxyTimeouts::default(),
            max_connections: 1000,
            buffer_size: 16 * 1024, // 16KB
        }
    }
}

/// HTTPS proxy server that forwards TLS traffic to P2P nodes
pub struct HttpsProxy {
    config: HttpsProxyConfig,
    registry: ProxyRegistry,
    endpoint: Endpoint,
}

impl HttpsProxy {
    /// Create a new HTTPS proxy
    pub fn new(config: HttpsProxyConfig, registry: ProxyRegistry, endpoint: Endpoint) -> Self {
        Self {
            config,
            registry,
            endpoint,
        }
    }

    /// Start the HTTPS proxy server
    pub async fn start(self: Arc<Self>) -> Result<()> {
        let listener = TcpListener::bind(self.config.bind_addr).await?;
        info!("HTTPS proxy listening on {}", self.config.bind_addr);

        // Connection semaphore to limit concurrent connections
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.max_connections));

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            let proxy = self.clone();
            let permit = semaphore.clone().acquire_owned().await.unwrap();

            counter("relay_https_connections_total").increment();
            gauge("relay_https_connections_active").increment();

            // Generate a correlation ID for this connection
            let correlation_id = CorrelationId::new();
            let correlation_id_for_span = correlation_id.clone();

            tokio::spawn(
                async move {
                    debug!("New connection from {}", peer_addr);

                    let start = std::time::Instant::now();
                    let result = proxy
                        .handle_connection(stream, peer_addr, correlation_id)
                        .await;
                    let duration = start.elapsed();

                    histogram("relay_https_connection_duration_seconds")
                        .observe(duration.as_secs_f64());
                    gauge("relay_https_connections_active").decrement();

                    if let Err(e) = result
                        && !matches!(e, crate::common::error::TlsForwardError::Io(_))
                    {
                        warn!("Error handling connection from {}: {}", peer_addr, e);
                        counter("relay_https_connection_errors").increment();
                    }

                    drop(permit);
                }
                .instrument(tracing::info_span!(
                    "https_connection",
                    %peer_addr,
                    correlation_id = %correlation_id_for_span,
                    otel.name = "relay_https_connection",
                    otel.kind = "SERVER",
                    trace_id = %correlation_id_for_span.trace_id(),
                    span_id = %correlation_id_for_span.span_id(),
                    traceparent = %correlation_id_for_span.to_traceparent()
                )),
            );
        }
    }

    /// Handle a single HTTPS connection
    #[instrument(
        name = "handle_https_connection",
        skip(self, stream),
        fields(
            peer_addr = %peer_addr,
            hostname = tracing::field::Empty,
            target_node = tracing::field::Empty,
            correlation_id = %correlation_id
        )
    )]
    async fn handle_connection(
        &self,
        mut stream: TcpStream,
        peer_addr: SocketAddr,
        correlation_id: CorrelationId,
    ) -> Result<()> {
        // Read enough data to extract SNI
        let mut initial_data = vec![0u8; 1024]; // Usually enough for ClientHello
        let n = timeout(
            self.config.timeouts.sni_read,
            stream.peek(&mut initial_data),
        )
        .await
        .map_err(|_| crate::common::error::TlsForwardError::Timeout("SNI read timeout".into()))??;

        initial_data.truncate(n);

        // Extract SNI hostname
        let hostname = extract_sni(&initial_data)?;
        debug!("SNI hostname: {} from {}", hostname, peer_addr);
        tracing::Span::current().record("hostname", &hostname);

        // Look up target node in registry
        let entry = self.registry.lookup(&hostname).await?;
        debug!("Found node {} for domain {}", entry.node_id, hostname);
        tracing::Span::current().record("target_node", entry.node_id.to_string());

        counter("relay_https_proxy_requests_total").increment();

        // Create new connection to target node
        let p2p_connection = self
            .endpoint
            .connect(entry.node_id, TLS_FORWARD_ALPN)
            .await
            .map_err(crate::common::error::TlsForwardError::P2pConnect)?;

        // Open bidirectional stream with timeout
        let (send_stream, recv_stream) =
            timeout(self.config.timeouts.connect, p2p_connection.open_bi())
                .await
                .map_err(|_| {
                    crate::common::error::TlsForwardError::Timeout(
                        "Failed to open P2P stream".into(),
                    )
                })??;
        let mut p2p_stream = CombinedStream::new(recv_stream, send_stream);

        info!("Proxying {} -> {} ({})", peer_addr, hostname, entry.node_id);

        // Forward traffic bidirectionally with idle timeout
        let result = timeout(
            self.config.timeouts.idle,
            StreamUtils::copy_bidirectional(&mut stream, &mut p2p_stream),
        )
        .await;

        // Connection will be closed when it goes out of scope
        match result {
            Ok(Ok((client_to_p2p, p2p_to_client))) => {
                debug!(
                    "Connection closed: {} -> {} ({} bytes, {} bytes)",
                    peer_addr, hostname, client_to_p2p, p2p_to_client
                );
                counter("relay_https_bytes_sent").add(p2p_to_client);
                counter("relay_https_bytes_received").add(client_to_p2p);
            }
            Ok(Err(e)) => {
                if !is_connection_closed_error(&e) {
                    error!("Error copying data: {}", e);
                    counter("relay_https_proxy_errors").increment();
                }
            }
            Err(_) => {
                warn!("Connection idle timeout: {} -> {}", peer_addr, hostname);
                counter("relay_https_proxy_errors").increment();
            }
        }

        Ok(())
    }

    /// Gracefully shutdown the proxy
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down HTTPS proxy");
        Ok(())
    }
}

/// Check if an error is just a connection closed error
fn is_connection_closed_error(e: &gate_p2p::stream::StreamError) -> bool {
    if let gate_p2p::stream::StreamError::Io(io_err) = e {
        matches!(
            io_err.kind(),
            std::io::ErrorKind::UnexpectedEof
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::BrokenPipe
        )
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HttpsProxyConfig::default();
        assert_eq!(config.bind_addr.port(), 443);
        assert_eq!(config.max_connections, 1000);
    }
}
