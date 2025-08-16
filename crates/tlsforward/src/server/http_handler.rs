//! HTTP protocol handler for serving REST APIs over Iroh P2P connections

use crate::server::dns_challenge::DnsChallengeManager;
use crate::server::registry::ProxyRegistry;
use crate::server::router::{ApiState, create_api_router};
use gate_core::tracing::metrics::{counter, gauge};
use gate_p2p::stream::CombinedStream;
use hyper_util::rt::TokioExecutor;
use hyper_util::server;
use iroh::{
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use std::sync::Arc;
use tower::Service;

/// HTTP protocol handler that serves REST APIs over P2P connections
pub struct TlsForwardHttpHandler {
    /// DNS challenge manager for ACME operations
    dns_manager: Arc<DnsChallengeManager>,
    /// Proxy registry for node registration
    registry: Arc<ProxyRegistry>,
    /// Domain suffix for TLS forward
    domain_suffix: String,
    /// The TLS forward server's own node ID
    tlsforward_node_id: iroh::NodeId,
}

impl TlsForwardHttpHandler {
    /// Create a new HTTP protocol handler
    pub fn new(
        dns_manager: Arc<DnsChallengeManager>,
        registry: Arc<ProxyRegistry>,
        domain_suffix: String,
        tlsforward_node_id: iroh::NodeId,
    ) -> Self {
        Self {
            dns_manager,
            registry,
            domain_suffix,
            tlsforward_node_id,
        }
    }
}

impl std::fmt::Debug for TlsForwardHttpHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsForwardHttpHandler").finish()
    }
}

impl ProtocolHandler for TlsForwardHttpHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let node_id = connection.remote_node_id()?;

        info!("HTTP connection from node {}", node_id);

        // Accept multiple bidirectional streams on this connection
        loop {
            // Add timeout to prevent hanging on accept_bi
            match tokio::time::timeout(std::time::Duration::from_secs(30), connection.accept_bi())
                .await
            {
                Ok(Ok((send_stream, recv_stream))) => {
                    debug!("Accepted HTTP stream from {}", node_id);

                    // Clone dependencies for the spawned task
                    let dns_manager = self.dns_manager.clone();
                    let registry = self.registry.clone();
                    let domain_suffix = self.domain_suffix.clone();
                    let tlsforward_node_id = self.tlsforward_node_id;

                    // Spawn a task to handle this HTTP request
                    tokio::spawn(async move {
                        // Track HTTP API requests
                        gauge("tlsforward_http_requests_active").increment();
                        counter("tlsforward_http_requests_total").increment();

                        let result = handle_http_stream(
                            recv_stream,
                            send_stream,
                            dns_manager,
                            registry,
                            domain_suffix,
                            node_id,
                            tlsforward_node_id,
                        )
                        .await;

                        gauge("tlsforward_http_requests_active").decrement();

                        if let Err(e) = result {
                            counter("tlsforward_http_requests_errors").increment();
                            error!("Error handling HTTP stream: {}", e);
                        }
                    });
                }
                Ok(Err(e)) => {
                    // Connection closed or error
                    debug!("HTTP connection closed: {}", e);
                    break;
                }
                Err(_) => {
                    // Timeout waiting for new stream
                    debug!("Timeout waiting for new HTTP stream from {}", node_id);
                    break;
                }
            }
        }

        Ok(())
    }
}

/// Handle a single HTTP request/response over a bidirectional stream
#[allow(clippy::too_many_arguments)]
#[instrument(
    name = "handle_http_stream",
    skip_all,
    fields(
        node_id = %node_id,
        protocol = "HTTP",
        domain_suffix = %domain_suffix
    )
)]
async fn handle_http_stream(
    recv_stream: iroh::endpoint::RecvStream,
    send_stream: iroh::endpoint::SendStream,
    dns_manager: Arc<DnsChallengeManager>,
    registry: Arc<ProxyRegistry>,
    domain_suffix: String,
    node_id: iroh::NodeId,
    tlsforward_node_id: iroh::NodeId,
) -> anyhow::Result<()> {
    // Combine the streams
    let stream = CombinedStream::new(recv_stream, send_stream);

    // Wrap the stream for hyper compatibility
    let io = hyper_util::rt::TokioIo::new(stream);

    // Create the API state
    let api_state = ApiState {
        dns_manager,
        registry,
        domain_suffix,
        tlsforward_node_id,
    };

    // Create the router and apply state
    let router = create_api_router()
        .layer(axum::extract::Extension(node_id))
        .with_state(api_state);

    // Create the service function
    let hyper_service =
        hyper::service::service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
            router.clone().call(request)
        });

    // Serve the connection using hyper's low-level API
    match server::conn::auto::Builder::new(TokioExecutor::new())
        .serve_connection(io, hyper_service)
        .await
    {
        Ok(()) => {
            debug!("HTTP connection completed successfully");
        }
        Err(e) => {
            error!("HTTP connection error: {}", e);
            return Err(anyhow::anyhow!("HTTP connection error: {}", e));
        }
    }

    Ok(())
}
