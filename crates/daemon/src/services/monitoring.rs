use gate_core::StateBackend;
use std::sync::Arc;
use tokio::task::JoinHandle;

use crate::services::{TlsForwardService, TlsForwardState, WebAuthnService};

/// Manages all monitoring tasks
pub struct MonitoringService {
    tasks: Vec<JoinHandle<()>>,
}

impl MonitoringService {
    /// Start all monitoring tasks
    pub fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    /// Start database pool metrics collection
    pub fn monitor_database_pool(&mut self, _state_backend: Arc<dyn StateBackend>) {
        // TODO: Add pool_metrics to StateBackend trait or use a different approach
        // For now, skip database pool monitoring in the refactored version
        debug!("Database pool monitoring not implemented for trait objects");
    }

    /// Monitor TLS forward state for WebAuthn updates
    pub fn monitor_webauthn_tlsforward(
        &mut self,
        service: Arc<TlsForwardService>,
        webauthn_service: Arc<WebAuthnService>,
    ) {
        let mut state_rx = service.subscribe();
        let mut last_domain: Option<String> = None;

        let handle = tokio::spawn(async move {
            while state_rx.changed().await.is_ok() {
                let state = state_rx.borrow().clone();
                if let TlsForwardState::Connected {
                    assigned_domain, ..
                } = state
                {
                    // Check if domain changed
                    if last_domain.as_ref() != Some(&assigned_domain) {
                        // Build the HTTPS origin URL for the assigned domain
                        let tlsforward_origin = format!("https://{assigned_domain}");

                        info!(
                            "TLS forward connected with domain: {}, updating WebAuthn allowed origins",
                            assigned_domain
                        );

                        // Add the TLS forward origin to WebAuthn allowed origins
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
                            last_domain = Some(assigned_domain);
                        }
                    }
                }
            }
        });

        self.tasks.push(handle);
    }

    /// Graceful shutdown of all monitoring tasks
    pub async fn shutdown(self) {
        info!("Shutting down monitoring tasks");
        for task in self.tasks {
            task.abort();
        }
    }
}

impl Default for MonitoringService {
    fn default() -> Self {
        Self::new()
    }
}
