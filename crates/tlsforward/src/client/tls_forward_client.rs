//! TLS forward client for daemon-tlsforward communication

use crate::TLSFORWARD_HTTP_ALPN;
use crate::common::{
    error::{Result, TlsForwardError},
    types::TlsForwardInfo,
    *,
};
use bytes::Bytes;
use futures::future::{AbortHandle, Abortable};
use gate_p2p::stream::CombinedStream;
use http_body_util::{BodyExt, Full};
use hyper::Method;
use hyper_util::rt::TokioIo;
use iroh::{Endpoint, NodeId};
use std::sync::Arc;
use std::time::Duration;

/// Client for all daemon-tlsforward communications
#[derive(Clone)]
pub struct TlsForwardClient {
    endpoint: Arc<Endpoint>,
    forwarder: NodeId,
}

impl TlsForwardClient {
    /// Create a new TLS forward client
    pub fn new(endpoint: Arc<Endpoint>, forwarder: NodeId) -> Self {
        Self {
            endpoint,
            forwarder,
        }
    }

    /// Register with the TLS forward service
    pub async fn register(&self) -> Result<(String, TlsForwardInfo)> {
        info!("Registering with TLS forward server at {}", self.forwarder);

        let request = RegistrationRequest {};
        let response: RegistrationResponse = self
            .request(Method::POST, "/register", Some(&request))
            .await?;

        debug!("TLS forward info: {:?}", response);

        Ok((response.domain, response.tlsforward_info))
    }

    /// Unregister from TLS forward service
    pub async fn unregister(&self) -> Result<()> {
        info!("Unregistering from TLS forward at {}", self.forwarder);

        self.request::<(), EmptyResponse>(Method::DELETE, "/unregister", None)
            .await?;

        info!("Successfully unregistered");
        Ok(())
    }

    /// Check registration status
    pub async fn status(&self) -> Result<StatusResponse> {
        debug!("Checking status with TLS forward");
        self.request(Method::GET, "/status", None::<&()>).await
    }

    /// Ping TLS forward service
    pub async fn ping(&self) -> Result<Duration> {
        let start = std::time::Instant::now();

        self.request::<(), EmptyResponse>(Method::POST, "/ping", None)
            .await?;

        let latency = start.elapsed();
        debug!("TLS forward ping latency: {:?}", latency);
        Ok(latency)
    }

    /// Create a DNS challenge
    pub async fn create_challenge(
        &self,
        domain: String,
        challenge: String,
        value: String,
    ) -> Result<CreateChallengeResponse> {
        let request = CreateChallengeRequest {
            domain,
            challenge,
            value,
        };

        let response = self
            .request(Method::POST, "/acme/challenge", Some(&request))
            .await?;

        info!("Created DNS challenge: {:?}", response);
        Ok(response)
    }

    /// Get challenge status
    pub async fn get_challenge_status(&self, id: &str) -> Result<ChallengeStatusResponse> {
        if id.is_empty() {
            return Err(TlsForwardError::Protocol(
                "Cannot get status for empty challenge ID".to_string(),
            ));
        }

        self.request(
            Method::GET,
            &format!("/acme/challenge/{id}/status"),
            None::<&()>,
        )
        .await
    }

    /// Delete a challenge
    pub async fn delete_challenge(&self, id: &str) -> Result<DeleteChallengeResponse> {
        if id.is_empty() {
            return Err(TlsForwardError::Protocol(
                "Cannot delete challenge with empty ID".to_string(),
            ));
        }

        let response = self
            .request(
                Method::DELETE,
                &format!("/acme/challenge/{id}"),
                None::<&()>,
            )
            .await?;

        info!("Deleted DNS challenge: {}", id);
        Ok(response)
    }

    /// Wait for DNS propagation
    pub async fn wait_for_dns_propagation(
        &self,
        id: &str,
        timeout: Duration,
        check_interval: Duration,
    ) -> Result<()> {
        // Validate challenge ID
        if id.is_empty() {
            return Err(TlsForwardError::Protocol(
                "Cannot wait for propagation with empty challenge ID".to_string(),
            ));
        }

        let start = std::time::Instant::now();

        loop {
            let status = self.get_challenge_status(id).await?;

            match &status.status {
                ChallengeStatus::Propagated => {
                    info!("DNS challenge {} is ready after {:?}", id, start.elapsed());
                    return Ok(());
                }
                ChallengeStatus::Pending => {
                    debug!(
                        "DNS challenge {} still pending (checks: {})",
                        id, status.checks
                    );
                }
                ChallengeStatus::Failed { error } => {
                    return Err(TlsForwardError::Protocol(format!(
                        "DNS challenge {id} failed: {error}"
                    )));
                }
            }

            if start.elapsed() > timeout {
                return Err(TlsForwardError::Protocol(format!(
                    "DNS propagation timeout after {timeout:?}"
                )));
            }

            tokio::time::sleep(check_interval).await;
        }
    }

    /// Make a typed HTTP request to the TLS forward server
    async fn request<Req, Res>(&self, method: Method, path: &str, body: Option<&Req>) -> Result<Res>
    where
        Req: serde::Serialize,
        Res: for<'de> serde::Deserialize<'de>,
    {
        // Create new connection to TLS forward server
        debug!("Creating new connection to TLS forward server");
        let connection = self
            .endpoint
            .connect(self.forwarder, TLSFORWARD_HTTP_ALPN)
            .await
            .map_err(TlsForwardError::P2pConnect)?;

        debug!("Connected, opening bidirectional stream");
        // Open bidirectional stream
        let (send_stream, recv_stream) = connection.open_bi().await?;
        debug!("Stream opened, creating HTTP connection");
        let stream = CombinedStream::new(recv_stream, send_stream);
        let io = TokioIo::new(stream);

        // Create HTTP client connection
        debug!("Starting HTTP handshake");
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|e| TlsForwardError::Protocol(format!("HTTP handshake failed: {e}")))?;

        debug!("HTTP handshake complete, spawning connection handler");

        // Create abortable connection handler
        let (abort_handle, abort_registration) = AbortHandle::new_pair();
        let conn_future = Abortable::new(conn, abort_registration);

        // Spawn the handler with abort support
        tokio::spawn(async move {
            match conn_future.await {
                Ok(Ok(())) => debug!("HTTP connection handler completed successfully"),
                Ok(Err(e)) => error!("HTTP connection handler error: {}", e),
                Err(e) => debug!("HTTP connection handler was aborted: {}", e),
            }
        });

        // Store abort handle to ensure cleanup on drop
        let _abort_handle = abort_handle;

        // Build request body
        let req_body = if let Some(body) = body {
            let json = serde_json::to_vec(body)?;
            Full::new(Bytes::from(json))
        } else {
            Full::new(Bytes::new())
        };

        // Build HTTP request
        debug!("Building HTTP request: {} {}", method, path);
        let request = hyper::Request::builder()
            .method(method)
            .uri(path)
            .header("Content-Type", "application/json")
            .header("Host", "tlsforward.private.internal")
            .body(req_body)
            .map_err(|e| TlsForwardError::Protocol(format!("Failed to build request: {e}")))?;

        // Send request
        debug!("Sending HTTP request");
        let response = sender
            .send_request(request)
            .await
            .map_err(|e| TlsForwardError::Protocol(format!("Failed to send request: {e}")))?;

        debug!("Got response with status: {}", response.status());

        // Check status
        let status = response.status();
        if !status.is_success() {
            let body = response
                .into_body()
                .collect()
                .await
                .map(|b| b.to_bytes())
                .unwrap_or_default();
            let error_text = String::from_utf8_lossy(&body);
            return Err(TlsForwardError::Protocol(format!(
                "HTTP {} {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown"),
                error_text
            )));
        }

        // Read and parse response
        let body_bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|e| TlsForwardError::Protocol(format!("Failed to read response: {e}")))?
            .to_bytes();

        // Connection will be closed when it goes out of scope

        if body_bytes.is_empty() {
            // Try to parse as unit type or empty object
            serde_json::from_str("{}").map_err(|e| {
                TlsForwardError::Protocol(format!("Failed to parse empty response: {e}"))
            })
        } else {
            serde_json::from_slice(&body_bytes)
                .map_err(|e| TlsForwardError::Protocol(format!("Failed to parse response: {e}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_challenge_request_serialization() {
        let request = CreateChallengeRequest {
            domain: "test.private.hellas.ai".to_string(),
            challenge: "_acme-challenge".to_string(),
            value: "test-value-123".to_string(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("test.private.hellas.ai"));
        assert!(json.contains("_acme-challenge"));
        assert!(json.contains("test-value-123"));
    }
}
