//! TLS forwarding handler for receiving traffic from relay

use gate_core::tracing::metrics::{counter, gauge, histogram};
use gate_http::server::HttpServer;
use gate_p2p::stream::CombinedStream;
use iroh::{
    endpoint::{Connection, RecvStream, SendStream},
    protocol::{AcceptError, ProtocolHandler},
};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;
use tracing::{debug, error, info, instrument, warn};

/// Trait for providing TLS acceptors
pub trait TlsAcceptorProvider: Send + Sync + Clone + 'static {
    /// Get the current TLS acceptor
    fn get_acceptor(&self) -> Pin<Box<dyn Future<Output = TlsAcceptor> + Send + '_>>;
}

/// Simple static TLS acceptor provider
impl TlsAcceptorProvider for TlsAcceptor {
    fn get_acceptor(&self) -> Pin<Box<dyn Future<Output = TlsAcceptor> + Send + '_>> {
        Box::pin(async { self.clone() })
    }
}

/// TLS forwarding handler that receives traffic from relay
#[derive(Clone)]
pub struct TlsForwardHandler<P> {
    tls_acceptor_provider: Arc<P>,
    http_server: Arc<HttpServer>,
    connection_timeout: Duration,
    connections_semaphore: Arc<Semaphore>,
    max_connections: usize,
}

impl<P: TlsAcceptorProvider> TlsForwardHandler<P> {
    /// Create a new TLS forwarding handler
    pub fn new(
        tls_acceptor_provider: P,
        http_server: Arc<HttpServer>,
        max_connections: usize,
        connection_timeout_secs: u64,
    ) -> Self {
        Self {
            tls_acceptor_provider: Arc::new(tls_acceptor_provider),
            http_server,
            connection_timeout: Duration::from_secs(connection_timeout_secs),
            connections_semaphore: Arc::new(Semaphore::new(max_connections)),
            max_connections,
        }
    }

    /// Handle a forwarded TLS connection
    #[instrument(
        name = "p2p.tls_connection",
        skip_all,
        fields(
            node_id = %node_id,
            correlation_id = tracing::field::Empty,
            connection_duration = tracing::field::Empty
        )
    )]
    async fn handle_connection(
        &self,
        send: SendStream,
        recv: RecvStream,
        node_id: iroh::NodeId,
    ) -> anyhow::Result<()> {
        // Try to acquire permit with warning if at capacity
        let _permit = match self.connections_semaphore.try_acquire() {
            Ok(permit) => permit,
            Err(_) => {
                warn!(
                    "Connection limit reached ({}/{}), waiting for available slot",
                    self.max_connections, self.max_connections
                );
                counter("relay_connections_rejected").increment();
                self.connections_semaphore
                    .acquire()
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to acquire connection permit: {}", e))?
            }
        };

        // Track connection metrics
        let start = Instant::now();
        gauge("relay_connections_active").increment();
        counter("relay_connections_total").increment();

        debug!(
            "Handling TLS forward connection from relay (node: {})",
            node_id
        );
        let active_connections =
            self.max_connections - self.connections_semaphore.available_permits();
        debug!(
            "Active connections: {}/{}",
            active_connections, self.max_connections
        );

        // Combine streams
        let stream = CombinedStream::new(recv, send);

        // Get current TLS acceptor
        let tls_acceptor = self.tls_acceptor_provider.get_acceptor().await;

        // Terminate TLS
        let tls_stream = timeout(self.connection_timeout, tls_acceptor.accept(stream))
            .await
            .map_err(|_| anyhow::anyhow!("TLS handshake timeout"))?
            .map_err(|e| anyhow::anyhow!("TLS handshake failed: {}", e))?;

        debug!("TLS handshake completed successfully");

        // Forward to HTTP server with P2P info with a timeout to prevent hanging
        let http_timeout = Duration::from_secs(300); // 5 minutes for HTTP handling
        let result = timeout(
            http_timeout,
            self.http_server
                .handle_p2p_stream(tls_stream, node_id, None),
        )
        .await
        .map_err(|_| {
            error!("HTTP handler timeout after {:?}", http_timeout);
            anyhow::anyhow!("HTTP handler timeout")
        })
        .and_then(|r| r);

        // Record connection duration in span
        let span = tracing::Span::current();
        span.record("connection_duration", format!("{:?}", start.elapsed()));

        // Record connection metrics
        gauge("relay_connections_active").decrement();
        let duration = start.elapsed();
        histogram("relay_connection_duration_seconds").observe(duration.as_secs_f64());

        match &result {
            Ok(_) => {
                counter("relay_connections_completed_success").increment();
            }
            Err(e) => {
                counter("relay_connections_completed_error").increment();
                error!("Connection error: {}", e);
            }
        }

        result
    }
}

impl<P: TlsAcceptorProvider> std::fmt::Debug for TlsForwardHandler<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsForwardHandler").finish()
    }
}

impl<P: TlsAcceptorProvider> ProtocolHandler for TlsForwardHandler<P> {
    #[instrument(
        name = "p2p.accept_connection", 
        skip_all,
        fields(
            remote_node_id = tracing::field::Empty
        )
    )]
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let handler = self.clone();
        let node_id = connection.remote_node_id()?;

        // Record node ID in span
        let span = tracing::Span::current();
        span.record("remote_node_id", node_id.to_string());

        info!("TLS forward handler: Accepted connection from {}", node_id);

        // Track spawned tasks
        let mut tasks = JoinSet::new();

        // Accept bidirectional streams with timeout
        loop {
            match timeout(Duration::from_secs(30), connection.accept_bi()).await {
                Ok(Ok((send, recv))) => {
                    let handler = handler.clone();
                    tasks.spawn(async move {
                        if let Err(e) = handler.handle_connection(send, recv, node_id).await {
                            error!("TLS forward connection error: {}", e);
                        }
                    });

                    // Clean up completed tasks periodically
                    while let Some(result) = tasks.try_join_next() {
                        if let Err(e) = result {
                            error!("Task join error: {}", e);
                            counter("relay_connections_task_panicked").increment();
                        }
                    }
                }
                Ok(Err(e)) => {
                    // Connection closed
                    debug!("Connection closed: {}", e);
                    break;
                }
                Err(_) => {
                    // Timeout - no new streams for 30 seconds
                    debug!("No new streams for 30 seconds, closing connection");
                    break;
                }
            }
        }

        // Wait for all spawned tasks to complete
        let pending_tasks = tasks.len();
        if pending_tasks > 0 {
            debug!(
                "Waiting for {} active stream handlers to complete",
                pending_tasks
            );
            gauge("relay_connections_cleanup_pending").set(pending_tasks as i64);

            while let Some(result) = tasks.join_next().await {
                if let Err(e) = result {
                    error!("Task join error during cleanup: {}", e);
                    counter("relay_connections_task_panicked").increment();
                }
            }

            gauge("relay_connections_cleanup_pending").set(0);
            info!("All stream handlers completed");
        }

        Ok(())
    }
}
