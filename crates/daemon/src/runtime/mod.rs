mod builder;
mod helpers;
mod inner;

use anyhow::Result;
use gate_http::AppState;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::info;

pub use builder::RuntimeBuilder;
pub use helpers::{
    build_daemon_router, create_p2p_endpoint, create_p2p_router, load_or_create_p2p_secret_key,
    request_letsencrypt_certificates, setup_cert_manager_client, setup_certificate_manager,
    spawn_webauthn_monitor, start_tlsforward_service,
};

use inner::RuntimeInner;

/// Simple runtime handle that hides all complexity
#[derive(Clone)]
pub struct Runtime {
    inner: Arc<RuntimeInner>,
    shutdown_tx: watch::Sender<bool>,
}

impl Runtime {
    /// Create a new runtime builder
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::default()
    }

    /// Start HTTP server (non-blocking)
    pub async fn serve(self) -> Result<()> {
        self.inner.serve(self.shutdown_tx.subscribe()).await
    }

    /// Get server address
    pub fn server_address(&self) -> String {
        format!(
            "{}:{}",
            self.inner.settings.server.host, self.inner.settings.server.port
        )
    }

    /// Get bootstrap token if available
    pub fn bootstrap_token(&self) -> Option<&str> {
        self.inner.bootstrap_token.as_deref()
    }

    /// Get bootstrap URL
    pub fn bootstrap_url(&self) -> Option<String> {
        self.bootstrap_token().map(|token| {
            format!(
                "http://localhost:{}/bootstrap/{}",
                self.inner.settings.server.port, token
            )
        })
    }

    /// Get the count of registered users
    pub fn user_count(&self) -> usize {
        self.inner.user_count
    }

    /// Check if TLS forward is enabled
    pub fn tlsforward_enabled(&self) -> bool {
        self.inner.tlsforward_service.is_some()
    }

    /// Get TLS forward status
    pub async fn tlsforward_status(&self) -> TlsForwardStatus {
        if let Some(service) = &self.inner.tlsforward_service {
            let state = service.subscribe().borrow().clone();
            match state {
                crate::services::TlsForwardState::Disconnected => TlsForwardStatus::Disconnected,
                crate::services::TlsForwardState::Connecting => TlsForwardStatus::Connecting,
                crate::services::TlsForwardState::Connected {
                    assigned_domain, ..
                } => TlsForwardStatus::Connected {
                    domain: assigned_domain,
                },
                crate::services::TlsForwardState::Error(error) => TlsForwardStatus::Error(error),
            }
        } else {
            TlsForwardStatus::Disabled
        }
    }

    /// Get app state (for advanced usage)
    pub fn app_state(&self) -> &AppState<crate::ServerState> {
        &self.inner.app_state
    }

    /// Start monitoring tasks
    pub async fn start_monitoring(&self) -> Vec<tokio::task::JoinHandle<()>> {
        self.inner.start_monitoring().await
    }

    /// Start metrics server if configured
    pub async fn start_metrics(&self) -> Result<Option<tokio::task::JoinHandle<()>>> {
        self.inner.start_metrics().await
    }

    /// Graceful shutdown
    pub async fn shutdown(self) {
        info!("Initiating runtime shutdown");
        let _ = self.shutdown_tx.send(true);

        // Shutdown TLS forward service if running
        if let Some(service) = &self.inner.tlsforward_service {
            info!("Shutting down TLS forward service");
            service.shutdown().await;
        }

        // Give services time to cleanup
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}

/// Simplified TLS forward status for external consumers
#[derive(Debug, Clone)]
pub enum TlsForwardStatus {
    Disabled,
    Disconnected,
    Connecting,
    Connected { domain: String },
    Error(String),
}

impl std::fmt::Display for TlsForwardStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(f, "disabled"),
            Self::Disconnected => write!(f, "disconnected"),
            Self::Connecting => write!(f, "connecting"),
            Self::Connected { domain } => write!(f, "connected to {domain}"),
            Self::Error(e) => write!(f, "error: {e}"),
        }
    }
}
