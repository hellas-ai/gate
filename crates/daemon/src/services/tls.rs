use anyhow::Result;
use gate_tlsforward::{CertificateManager, TlsForwardClient};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{services::TlsForwardService, tls_reload::ReloadableTlsAcceptor};

/// Manages TLS certificates and acceptors
#[derive(Clone)]
pub struct TlsManager {
    certificate_manager: Arc<Mutex<CertificateManager>>,
    reloadable_acceptor: Arc<ReloadableTlsAcceptor>,
}

impl TlsManager {
    /// Create a new TLS manager
    pub async fn new(data_dir: &Path, initial_domains: Vec<String>) -> Result<Self> {
        // Create certificate manager
        let cert_manager = Arc::new(Mutex::new(CertificateManager::new(data_dir.to_path_buf())));

        // Get domains for certificate
        let mut domains = initial_domains;

        // Only add localhost if no other domains are configured
        if domains.is_empty() {
            domains.push("localhost".to_string());
        }

        // Get or create TLS acceptor
        let acceptor = cert_manager
            .lock()
            .await
            .get_or_create_tls_acceptor(&domains)
            .await?;
        let reloadable_acceptor = Arc::new(ReloadableTlsAcceptor::new(acceptor));

        Ok(Self {
            certificate_manager: cert_manager,
            reloadable_acceptor,
        })
    }

    /// Get the reloadable TLS acceptor
    pub fn acceptor(&self) -> Arc<ReloadableTlsAcceptor> {
        self.reloadable_acceptor.clone()
    }

    /// Set TLS forward client for ACME challenges
    pub async fn set_tls_forward_client(
        &self,
        endpoint: Arc<gate_p2p::Endpoint>,
        node_id: gate_p2p::NodeId,
    ) {
        let client = TlsForwardClient::new(endpoint, node_id);
        self.certificate_manager
            .lock()
            .await
            .set_tls_forward_client(client);
        debug!("Certificate manager configured with TLS forward client");
    }

    /// Request Let's Encrypt certificates
    pub async fn request_certificates(&self, domains: Vec<String>, email: &str) -> Result<()> {
        if domains.is_empty() {
            return Ok(());
        }

        info!("Requesting certificates for configured domains");
        for domain in &domains {
            info!("Checking certificate for domain: https://{}", domain);

            let cert_mgr = self.certificate_manager.lock().await;
            if !cert_mgr.has_certificate(domain).await {
                info!("Requesting new certificate for https://{}", domain);
                match cert_mgr.request_certificate(domain, email).await {
                    Ok(()) => {
                        info!("Successfully obtained certificate for {}", domain);
                        // Reload TLS acceptor with new certificates
                        if let Ok(new_acceptor) =
                            cert_mgr.get_or_create_tls_acceptor(&domains).await
                        {
                            self.reloadable_acceptor.reload(new_acceptor).await;
                            info!(
                                "Reloaded TLS acceptor with new certificates for domains: {:?}",
                                domains
                            );
                        }
                    }
                    Err(e) => {
                        error!("Failed to obtain certificate for https://{}: {}", domain, e)
                    }
                }
            } else {
                info!("Certificate already exists for https://{}", domain);
            }
        }

        Ok(())
    }

    /// Monitor TLS forward service for certificate updates
    pub async fn monitor_tlsforward_certificates(
        &self,
        service: Arc<TlsForwardService>,
        letsencrypt_domains: Vec<String>,
    ) {
        let cert_manager = self.certificate_manager.clone();
        let acceptor = self.reloadable_acceptor.clone();

        let mut state_rx = service.subscribe();
        let mut last_domain: Option<String> = None;

        tokio::spawn(async move {
            while state_rx.changed().await.is_ok() {
                let state = state_rx.borrow().clone();
                if let crate::services::TlsForwardState::Connected {
                    assigned_domain, ..
                } = state
                    && last_domain.as_ref() != Some(&assigned_domain)
                {
                    info!("TLS forward connected with new domain: {}", assigned_domain);
                    info!(
                        "TLS forward domain {} detected. Manual config update required for Let's Encrypt.",
                        assigned_domain
                    );

                    // Check if we have a certificate for this domain
                    let cert_mgr = cert_manager.lock().await;
                    if cert_mgr.has_certificate(&assigned_domain).await {
                        info!(
                            "Found existing certificate for TLS forward domain: {}",
                            assigned_domain
                        );

                        // Reload TLS acceptor with TLS forward domain
                        let mut domains = vec![assigned_domain.clone()];
                        domains.extend(letsencrypt_domains.clone());

                        if let Ok(new_acceptor) =
                            cert_mgr.get_or_create_tls_acceptor(&domains).await
                        {
                            acceptor.reload(new_acceptor).await;
                            info!(
                                "Reloaded TLS acceptor with TLS forward domain: {}",
                                assigned_domain
                            );
                            last_domain = Some(assigned_domain);
                        }
                    } else {
                        info!(
                            "No certificate found for TLS forward domain: {}, will be requested later",
                            assigned_domain
                        );
                        last_domain = Some(assigned_domain);
                    }
                }
            }
        });
    }
}
