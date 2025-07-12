//! Certificate management for Let's Encrypt integration

use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::common::ChallengeStatus;

use instant_acme::{
    Account, AccountCredentials, ChallengeType, Identifier, LetsEncrypt, NewAccount, NewOrder,
    OrderStatus,
};

/// Certificate manager for handling Let's Encrypt certificates
pub struct CertificateManager {
    /// Path to store certificates
    cert_path: PathBuf,
    /// Path to store account credentials
    account_path: PathBuf,
    /// TLS forward client for DNS challenges
    tls_forward_client: Option<super::TlsForwardClient>,
    /// Cached account
    account: RwLock<Option<Arc<Account>>>,
}

impl CertificateManager {
    /// Create a new certificate manager
    pub fn new(data_dir: PathBuf) -> Self {
        let cert_path = data_dir.join("certificates");
        let account_path = data_dir.join("acme");

        // Ensure directories exist
        std::fs::create_dir_all(&cert_path).ok();
        std::fs::create_dir_all(&account_path).ok();

        Self {
            cert_path,
            account_path,
            tls_forward_client: None,
            account: RwLock::new(None),
        }
    }

    /// Set the TLS forward client for DNS challenges
    pub fn set_tls_forward_client(&mut self, client: super::TlsForwardClient) {
        self.tls_forward_client = Some(client);
    }

    /// Get or create ACME account
    async fn get_or_create_account(&self, email: &str) -> Result<Arc<Account>> {
        // Check if we have a cached account
        {
            let account_lock = self.account.read().await;
            if let Some(account) = account_lock.as_ref() {
                return Ok(account.clone());
            }
        }

        // Try to load from disk
        let creds_path = self.account_path.join("account.json");
        let account = if creds_path.exists() {
            debug!("Loading ACME account from disk");
            let creds_json = tokio::fs::read_to_string(&creds_path).await?;
            let creds: AccountCredentials = serde_json::from_str(&creds_json)?;
            Account::builder()?.from_credentials(creds).await?
        } else {
            info!("Creating new ACME account for {}", email);
            let contacts = [format!("mailto:{email}")];
            let (account, creds) = Account::builder()?
                .create(
                    &NewAccount {
                        contact: &contacts.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                        terms_of_service_agreed: true,
                        only_return_existing: false,
                    },
                    LetsEncrypt::Production.url().to_string(),
                    None,
                )
                .await?;

            // Save credentials
            let creds_json = serde_json::to_string_pretty(&creds)?;
            tokio::fs::write(&creds_path, creds_json).await?;

            account
        };

        let account = Arc::new(account);

        // Cache the account
        {
            let mut account_lock = self.account.write().await;
            *account_lock = Some(account.clone());
        }

        Ok(account)
    }

    /// Request a certificate for a domain
    pub async fn request_certificate(&self, domain: &str, email: &str) -> Result<()> {
        let tls_forward_client = self
            .tls_forward_client
            .as_ref()
            .context("TLS forward client not configured")?;

        info!("Requesting certificate for domain: {}", domain);

        // Get or create account
        let account = self.get_or_create_account(email).await?;

        // Create order
        let identifier = Identifier::Dns(domain.to_string());
        debug!("Creating ACME order for identifier: {:?}", identifier);
        let mut order = account.new_order(&NewOrder::new(&[identifier])).await?;
        debug!("Order created: {:?}", order.state());

        // Get authorizations
        let mut authorizations = order.authorizations();

        let mut authz = authorizations
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("No authorizations found"))??;

        let mut challenge = authz
            .challenge(ChallengeType::Dns01)
            .ok_or_else(|| anyhow::anyhow!("No DNS-01 challenge found"))?;

        let dns_value = challenge.key_authorization().dns_value();

        info!("Creating DNS challenge for domain: {}", domain);
        info!("DNS challenge value: {}", dns_value);

        // Create DNS challenge through TLS forward server
        let challenge_response = tls_forward_client
            .create_challenge(domain.to_string(), "_acme-challenge".to_string(), dns_value)
            .await
            .context("Failed to create challenge with TLS forward server")?;

        info!("Challenge response: {:?}", challenge_response);

        // Validate that we got a valid challenge ID
        if challenge_response.id.is_empty() {
            return Err(anyhow::anyhow!(
                "Received empty challenge ID from TLS forward server"
            ));
        }

        // Check if the challenge was created successfully
        if let ChallengeStatus::Failed { error } = &challenge_response.status {
            return Err(anyhow::anyhow!("Challenge creation failed: {}", error));
        }

        // Wait for DNS propagation
        info!(
            "Waiting for DNS propagation for challenge ID: {}",
            challenge_response.id
        );
        tls_forward_client
            .wait_for_dns_propagation(
                &challenge_response.id,
                Duration::from_secs(300), // 5 minute timeout
                Duration::from_secs(5),   // 5 second check interval
            )
            .await
            .inspect_err(|_e| {
                // Try to clean up the challenge on error
                let challenge_id = challenge_response.id.clone();
                let tls_forward_client = tls_forward_client.clone();
                tokio::spawn(async move {
                    if let Err(cleanup_err) =
                        tls_forward_client.delete_challenge(&challenge_id).await
                    {
                        warn!(
                            "Failed to clean up challenge {} after error: {}",
                            challenge_id, cleanup_err
                        );
                    }
                });
            })?;

        // Notify ACME server that challenge is ready
        info!("Notifying ACME server that challenge is ready");
        challenge.set_ready().await?;

        // Wait for challenge validation
        let mut attempts = 0;
        let max_attempts = 30;

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            order.refresh().await?;
            match order.state().status {
                OrderStatus::Ready => {
                    info!("Order is ready for finalization");
                    break;
                }
                OrderStatus::Invalid => {
                    error!("Order validation failed. Order state: {:?}", order.state());
                    // Try to get more details about the failure
                    let mut authz_iter = order.authorizations();
                    let mut idx = 0;
                    while let Some(Ok(authz)) = authz_iter.next().await {
                        error!("Authorization {}: status={:?}", idx, authz.status);
                        for chall in &authz.challenges {
                            error!(
                                "  Challenge type={:?} status={:?} error={:?}",
                                chall.r#type, chall.status, chall.error
                            );
                        }
                        idx += 1;
                    }
                    // Clean up DNS record
                    tls_forward_client
                        .delete_challenge(&challenge_response.id)
                        .await
                        .ok();
                    return Err(anyhow::anyhow!("Order validation failed"));
                }
                OrderStatus::Valid => {
                    info!("Order already valid");
                    break;
                }
                _ => {
                    debug!("Order status: {:?}", order.state().status);
                }
            }

            attempts += 1;
            if attempts >= max_attempts {
                // Clean up DNS record
                tls_forward_client
                    .delete_challenge(&challenge_response.id)
                    .await
                    .ok();
                return Err(anyhow::anyhow!("Challenge validation timeout"));
            }
        }

        // Clean up DNS record
        info!("Cleaning up DNS challenge");
        tls_forward_client
            .delete_challenge(&challenge_response.id)
            .await?;

        // Finalize order - instant-acme generates the key for us
        let private_key_pem = order.finalize().await?;

        // Wait for certificate
        let cert_chain_pem = loop {
            match order.certificate().await? {
                Some(cert_chain_pem) => break cert_chain_pem,
                None => tokio::time::sleep(std::time::Duration::from_secs(1)).await,
            }
        };

        // Save certificate and key
        let cert_dir = self.cert_path.join(domain);
        tokio::fs::create_dir_all(&cert_dir).await?;

        let cert_path = cert_dir.join("fullchain.pem");
        let key_path = cert_dir.join("key.pem");

        // Save full chain (cert + intermediates)
        tokio::fs::write(&cert_path, &cert_chain_pem).await?;

        // Save private key
        tokio::fs::write(&key_path, &private_key_pem).await?;

        info!("Certificate saved to {}", cert_dir.display());
        Ok(())
    }

    /// Check if a certificate exists for a domain
    pub async fn has_certificate(&self, domain: &str) -> bool {
        let cert_path = self.cert_path.join(domain).join("fullchain.pem");
        cert_path.exists()
    }

    /// Get certificate paths for a domain
    pub fn get_certificate_paths(&self, domain: &str) -> Option<CertificatePaths> {
        let cert_dir = self.cert_path.join(domain);
        let cert_path = cert_dir.join("fullchain.pem");
        let key_path = cert_dir.join("key.pem");

        if cert_path.exists() && key_path.exists() {
            Some(CertificatePaths {
                cert: cert_path,
                key: key_path,
                chain: None, // fullchain.pem already includes the chain
            })
        } else {
            None
        }
    }

    /// Create a TLS acceptor from available certificates or generate self-signed
    pub async fn get_or_create_tls_acceptor(
        &self,
        domains: &[String],
    ) -> Result<tokio_rustls::TlsAcceptor> {
        use tokio_rustls::rustls::{
            ServerConfig,
            pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
        };

        // Try to find existing certificate for any of the domains
        for domain in domains {
            if let Some(paths) = self.get_certificate_paths(domain)
                && paths.cert.exists()
                && paths.key.exists()
            {
                info!("Using existing certificate for domain: https://{}", domain);

                // Load certificate and key
                let certs = CertificateDer::pem_file_iter(&paths.cert)
                    .context("Failed to load certificate")?
                    .map(|cert| cert.context("Failed to parse certificate"))
                    .collect::<Result<Vec<_>>>()?;

                let key = PrivateKeyDer::from_pem_file(&paths.key)
                    .context("Failed to load private key")?;

                // Create server config
                let config = ServerConfig::builder()
                    .with_no_client_auth()
                    .with_single_cert(certs, key)
                    .context("Failed to create TLS config")?;

                return Ok(tokio_rustls::TlsAcceptor::from(Arc::new(config)));
            }
        }

        // No certificate found, generate self-signed
        warn!(
            "No certificates found for domains {:?}, generating self-signed certificate",
            domains
        );
        self.generate_self_signed_tls_acceptor(
            domains.first().map(|s| s.as_str()).unwrap_or("localhost"),
        )
    }

    /// Generate a self-signed certificate
    fn generate_self_signed_tls_acceptor(&self, domain: &str) -> Result<tokio_rustls::TlsAcceptor> {
        use rcgen::{CertifiedKey, generate_simple_self_signed};
        use tokio_rustls::rustls::{ServerConfig, pki_types::PrivateKeyDer};

        // Generate self-signed certificate
        let subject_alt_names = vec![domain.to_string(), "localhost".to_string()];
        let CertifiedKey { cert, signing_key } = generate_simple_self_signed(subject_alt_names)
            .context("Failed to generate self-signed certificate")?;

        let cert_der = cert.der().clone();
        let key_der = PrivateKeyDer::Pkcs8(signing_key.serialize_der().into());

        // Create server config
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der], key_der)
            .context("Failed to create TLS config")?;

        debug!("Generated self-signed certificate for domain: {}", domain);
        Ok(tokio_rustls::TlsAcceptor::from(Arc::new(config)))
    }
}

/// Paths to certificate files
#[derive(Debug, Clone)]
pub struct CertificatePaths {
    pub cert: PathBuf,
    pub key: PathBuf,
    pub chain: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_certificate_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let manager = CertificateManager::new(temp_dir.path().to_path_buf());

        assert!(manager.cert_path.exists());
        assert!(manager.account_path.exists());
    }

    #[test]
    fn test_certificate_paths() {
        let temp_dir = TempDir::new().unwrap();
        let manager = CertificateManager::new(temp_dir.path().to_path_buf());

        // No certificate should exist
        assert!(manager.get_certificate_paths("test.example.com").is_none());

        // Create fake certificate files
        let cert_dir = manager.cert_path.join("test.example.com");
        std::fs::create_dir_all(&cert_dir).unwrap();
        std::fs::write(cert_dir.join("fullchain.pem"), "cert").unwrap();
        std::fs::write(cert_dir.join("key.pem"), "key").unwrap();

        // Now paths should be found
        let paths = manager.get_certificate_paths("test.example.com").unwrap();
        assert!(paths.cert.exists());
        assert!(paths.key.exists());
        assert!(paths.chain.is_none());
    }
}
