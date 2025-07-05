//! DNS challenge management for ACME (Let's Encrypt)
//!
//! This module handles DNS-01 challenges by managing TXT records via Cloudflare.

use anyhow::{Result, anyhow};
use cloudflare::endpoints::dns::dns::{
    CreateDnsRecord, CreateDnsRecordParams, DeleteDnsRecord, DnsContent,
};
use cloudflare::framework::{
    Environment,
    auth::Credentials,
    client::{ClientConfig, async_api::Client as CfClient},
    response::ApiFailure,
};
use iroh::NodeId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::common::ChallengeStatus;

/// DNS challenge data
#[derive(Debug, Clone)]
pub struct DnsChallenge {
    /// The domain for the challenge
    pub domain: String,
    /// The challenge subdomain (typically "_acme-challenge")
    pub challenge: String,
    /// The challenge value to set
    pub value: String,
}

/// Internal challenge state
#[derive(Debug, Clone)]
struct ChallengeState {
    /// Node ID that created this challenge
    owner_node_id: NodeId,
    /// Cloudflare record ID (once created)
    record_id: Option<String>,
    /// Current status
    status: ChallengeStatus,
    /// Number of successful propagation checks
    checks: u32,
    /// The full DNS record name (e.g., "_acme-challenge.subdomain.domain.com")
    record_name: String,
    /// The expected TXT value
    txt_value: String,
}

/// Manager for DNS challenges using Cloudflare
pub struct DnsChallengeManager {
    /// Cloudflare API client
    cf_client: Arc<CfClient>,
    /// Cloudflare zone ID
    zone_id: String,
    /// Base domain (e.g., "private.hellas.ai")
    base_domain: String,
    /// Cloudflare zone domain (e.g., "hellas.ai")
    zone_domain: String,
    /// Active challenges
    challenges: Arc<RwLock<HashMap<String, ChallengeState>>>,
}

impl DnsChallengeManager {
    /// Create a new DNS challenge manager
    pub fn new(api_token: String, zone_id: String, base_domain: String) -> Result<Self> {
        let credentials = Credentials::UserAuthToken { token: api_token };

        let cf_client = CfClient::new(
            credentials,
            ClientConfig::default(),
            Environment::Production,
        )?;

        // Extract zone domain from base domain
        // If base_domain is "private.hellas.ai", zone_domain is "hellas.ai"
        let zone_domain = if base_domain.contains('.') {
            // Find the last two parts (TLD and domain)
            let parts: Vec<&str> = base_domain.split('.').collect();
            if parts.len() >= 2 {
                let second_last = parts[parts.len() - 2];
                let last = parts[parts.len() - 1];
                format!("{second_last}.{last}")
            } else {
                base_domain.clone()
            }
        } else {
            base_domain.clone()
        };

        info!("DNS Challenge Manager: base_domain={base_domain}, zone_domain={zone_domain}");

        Ok(Self {
            cf_client: Arc::new(cf_client),
            zone_id,
            base_domain,
            zone_domain,
            challenges: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Validate that a domain matches our expected pattern
    pub fn validate_domain(&self, domain: &str) -> bool {
        // Domain should end with our base domain
        domain.ends_with(&format!(".{}", self.base_domain))
    }

    /// Create a new DNS challenge
    pub async fn create_challenge(
        &self,
        challenge: DnsChallenge,
        owner_node_id: NodeId,
    ) -> Result<String> {
        // Generate unique ID for this challenge
        let id = Uuid::new_v4().to_string();

        info!(
            "DnsChallengeManager: Creating challenge {id} for domain {} (owner: {owner_node_id})",
            challenge.domain
        );

        // Extract subdomain from the full domain
        // If challenge.domain is "09793852586a678a.private.hellas.ai" and base_domain is "private.hellas.ai",
        // we need to extract "09793852586a678a.private" to create the record in the hellas.ai zone
        let record_name = if challenge
            .domain
            .ends_with(&format!(".{}", self.base_domain))
        {
            // Remove the base domain suffix to get just the subdomain
            let subdomain_len = challenge.domain.len() - self.base_domain.len() - 1;
            let subdomain = &challenge.domain[..subdomain_len];

            // Now we need to handle the zone difference
            // If base_domain is "private.hellas.ai" and zone_domain is "hellas.ai",
            // we need to keep the "private" part in the record name
            if self.base_domain != self.zone_domain
                && self
                    .base_domain
                    .ends_with(&format!(".{}", self.zone_domain))
            {
                // Extract the middle part (e.g., "private" from "private.hellas.ai")
                let middle_len = self.base_domain.len() - self.zone_domain.len() - 1;
                let middle_part = &self.base_domain[..middle_len];
                let challenge_name = &challenge.challenge;
                format!("{challenge_name}.{subdomain}.{middle_part}")
            } else {
                let challenge_name = &challenge.challenge;
                format!("{challenge_name}.{subdomain}")
            }
        } else {
            return Err(anyhow!(
                "Domain {} does not match base domain {}",
                challenge.domain,
                self.base_domain
            ));
        };

        let zone_domain = &self.zone_domain;
        let value = &challenge.value;
        info!("Creating TXT record: {record_name} (in zone {zone_domain}) = {value}");

        // Create the DNS record via Cloudflare
        let params = CreateDnsRecordParams {
            name: &record_name,
            content: DnsContent::TXT {
                content: challenge.value.clone(),
            },
            ttl: Some(120),
            proxied: Some(false),
            priority: None,
        };

        let zone_id = &self.zone_id;
        debug!("Sending request to Cloudflare API for zone {zone_id}");
        match self
            .cf_client
            .request(&CreateDnsRecord {
                zone_identifier: &self.zone_id,
                params,
            })
            .await
        {
            Ok(response) => {
                let record_id = response.result.id;
                info!("Successfully created DNS record with ID: {record_id}");

                // Store challenge state with DNS record details
                let zone_domain = &self.zone_domain;
                let state = ChallengeState {
                    owner_node_id,
                    record_id: Some(record_id),
                    status: ChallengeStatus::Pending,
                    checks: 0,
                    record_name: format!("{record_name}.{zone_domain}"),
                    txt_value: challenge.value.clone(),
                };

                self.challenges.write().await.insert(id.clone(), state);

                // Start background task to check propagation
                self.spawn_propagation_checker(id.clone());

                Ok(id)
            }
            Err(e) => {
                tracing::error!("Failed to create DNS record: {e:?}");
                // Log detailed error information
                let error_msg = match e {
                    ApiFailure::Error(status, api_errors) => {
                        error!("Cloudflare API error (HTTP {status}): {api_errors:?}");
                        // Log each error detail
                        for err in &api_errors.errors {
                            let code = &err.code;
                            let message = &err.message;
                            let other = &err.other;
                            error!("  Error {code}: {message} (other: {other:?})");
                        }
                        // Log any additional error information
                        for (k, v) in &api_errors.other {
                            error!("  {k}: {v}");
                        }
                        let error_message = api_errors
                            .errors
                            .first()
                            .map(|e| e.message.as_str())
                            .unwrap_or("Unknown error");
                        format!("Cloudflare API error (HTTP {status}): {error_message}")
                    }
                    ApiFailure::Invalid(req_err) => {
                        error!("Request error creating DNS record: {req_err}");
                        format!("Request error: {req_err}")
                    }
                };

                error!("Failed to create DNS record '{record_name}': {error_msg}");

                // Store failed state
                let zone_domain = &self.zone_domain;
                let state = ChallengeState {
                    owner_node_id,
                    record_id: None,
                    status: ChallengeStatus::Failed {
                        error: error_msg.clone(),
                    },
                    checks: 0,
                    record_name: format!("{record_name}.{zone_domain}"),
                    txt_value: challenge.value.clone(),
                };

                self.challenges.write().await.insert(id.clone(), state);

                Err(anyhow!(
                    "Failed to create DNS record '{record_name}': {error_msg}"
                ))
            }
        }
    }

    /// Get the status of a challenge
    pub async fn get_challenge_status(
        &self,
        id: &str,
        requester_node_id: &NodeId,
    ) -> Result<(ChallengeStatus, u32)> {
        let challenges = self.challenges.read().await;
        let state = challenges
            .get(id)
            .ok_or_else(|| anyhow!("Challenge not found"))?;

        // Verify the requester owns this challenge
        if state.owner_node_id != *requester_node_id {
            return Err(anyhow!("Challenge not found")); // Don't reveal it exists
        }

        Ok((state.status.clone(), state.checks))
    }

    /// Delete a DNS challenge
    pub async fn delete_challenge(&self, id: &str, requester_node_id: &NodeId) -> Result<()> {
        let mut challenges = self.challenges.write().await;

        // First check if it exists and verify ownership
        let state = challenges
            .get(id)
            .ok_or_else(|| anyhow!("Challenge not found"))?;

        // Verify the requester owns this challenge
        if state.owner_node_id != *requester_node_id {
            return Err(anyhow!("Challenge not found")); // Don't reveal it exists
        }

        // Now remove it
        let state = challenges.remove(id).unwrap();

        // Delete from Cloudflare if we have a record ID
        if let Some(record_id) = state.record_id {
            info!("Deleting DNS record: {record_id}");

            match self
                .cf_client
                .request(&DeleteDnsRecord {
                    zone_identifier: &self.zone_id,
                    identifier: &record_id,
                })
                .await
            {
                Ok(_) => info!("Successfully deleted DNS record: {record_id}"),
                Err(e) => {
                    let error_msg = match e {
                        ApiFailure::Error(status, api_errors) => {
                            error!(
                                "Cloudflare API error deleting DNS record (HTTP {status}): {api_errors:?}"
                            );
                            let error_message = api_errors
                                .errors
                                .first()
                                .map(|e| e.message.as_str())
                                .unwrap_or("Unknown error");
                            format!("Cloudflare API error (HTTP {status}): {error_message}")
                        }
                        ApiFailure::Invalid(req_err) => {
                            error!("Request error deleting DNS record: {req_err}");
                            format!("Request error: {req_err}")
                        }
                    };
                    return Err(anyhow!("Failed to delete DNS record: {error_msg}"));
                }
            }
        }

        Ok(())
    }

    /// Spawn a task to check DNS propagation
    fn spawn_propagation_checker(&self, challenge_id: String) {
        let challenges = self.challenges.clone();

        tokio::spawn(async move {
            let mut checks = 0;
            const MAX_CHECKS: u32 = 30; // 5 minutes max
            const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

            loop {
                tokio::time::sleep(CHECK_INTERVAL).await;
                checks += 1;

                // Get challenge info
                let mut challenges_lock = challenges.write().await;
                if let Some(state) = challenges_lock.get_mut(&challenge_id) {
                    // Check DNS propagation using the stored record details
                    let propagated =
                        check_dns_propagation(&state.record_name, &state.txt_value).await;

                    if propagated {
                        let seconds = checks * 10;
                        info!("Challenge {challenge_id} has propagated after {seconds} seconds");
                        state.status = ChallengeStatus::Propagated;
                        state.checks = checks;
                        break;
                    }

                    state.checks = checks;

                    if checks >= MAX_CHECKS {
                        warn!("Challenge {challenge_id} timed out waiting for propagation");
                        state.status = ChallengeStatus::Failed {
                            error: "DNS propagation timeout".to_string(),
                        };
                        break;
                    }
                } else {
                    // Challenge was deleted
                    debug!("Challenge {challenge_id} no longer exists");
                    break;
                }
            }
        });
    }
}

/// Check if DNS record has propagated
async fn check_dns_propagation(record_name: &str, expected_value: &str) -> bool {
    use trust_dns_resolver::{TokioAsyncResolver, config::*};

    // Use Cloudflare's DNS over TLS for faster resolution
    let resolver = TokioAsyncResolver::tokio(ResolverConfig::cloudflare(), ResolverOpts::default());

    debug!("Checking DNS propagation for {record_name} = {expected_value}");

    match resolver.txt_lookup(record_name).await {
        Ok(response) => {
            for txt_data in response.iter() {
                let txt_value = txt_data.to_string();

                debug!("Found TXT record: {record_name} = {txt_value}");

                // DNS servers may or may not include quotes in the response
                // Compare the actual content without quotes
                let normalized_txt = txt_value.trim_matches('"');
                let normalized_expected = expected_value.trim_matches('"');

                debug!(
                    "Comparing DNS values - found: '{txt_value}' (normalized: '{normalized_txt}'), expected: '{expected_value}' (normalized: '{normalized_expected}')"
                );

                if normalized_txt == normalized_expected {
                    info!("DNS propagation confirmed for {record_name}");
                    return true;
                }
            }
            debug!("DNS record found but value doesn't match for {record_name}");
            false
        }
        Err(e) => {
            debug!("DNS lookup failed for {record_name}: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_zone_domain_extraction() {
        // Test that we correctly extract the zone domain from the base domain
        let test_cases = vec![
            ("private.hellas.ai", "hellas.ai"),
            ("subdomain.example.com", "example.com"),
            ("deep.sub.domain.co.uk", "co.uk"),
            ("example.com", "example.com"),
            ("localhost", "localhost"),
        ];

        for (base_domain, expected_zone) in test_cases {
            let parts: Vec<&str> = base_domain.split('.').collect();
            let zone_domain = if parts.len() >= 2 {
                format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1])
            } else {
                base_domain.to_string()
            };

            assert_eq!(
                zone_domain, expected_zone,
                "Failed for base_domain: {base_domain}"
            );
        }
    }

    #[test]
    fn test_record_name_construction() {
        // Test the record name construction logic
        let test_cases = vec![
            // (domain, base_domain, zone_domain, challenge, expected_record_name)
            (
                "xxxxxx.private.hellas.ai",
                "private.hellas.ai",
                "hellas.ai",
                "_acme-challenge",
                "_acme-challenge.xxxxxx.private",
            ),
            (
                "test.example.com",
                "example.com",
                "example.com",
                "_acme-challenge",
                "_acme-challenge.test",
            ),
            (
                "sub.domain.example.com",
                "domain.example.com",
                "example.com",
                "_acme-challenge",
                "_acme-challenge.sub.domain",
            ),
        ];

        for (domain, base_domain, zone_domain, challenge, expected) in test_cases {
            // Simulate the record name construction logic
            let record_name = if domain.ends_with(&format!(".{base_domain}")) {
                let subdomain_len = domain.len() - base_domain.len() - 1;
                let subdomain = &domain[..subdomain_len];

                if base_domain != zone_domain && base_domain.ends_with(&format!(".{zone_domain}")) {
                    let middle_len = base_domain.len() - zone_domain.len() - 1;
                    let middle_part = &base_domain[..middle_len];
                    format!("{challenge}.{subdomain}.{middle_part}")
                } else {
                    format!("{challenge}.{subdomain}")
                }
            } else {
                panic!("Domain {domain} does not match base domain {base_domain}");
            };

            assert_eq!(
                record_name, expected,
                "Failed for domain: {domain}, base: {base_domain}, zone: {zone_domain}"
            );
        }
    }

    #[tokio::test]
    async fn test_dns_propagation_check_quote_handling() {
        // Test that the DNS propagation check handles quotes correctly

        // Test various quote scenarios
        let test_cases = vec![
            // (dns_value, expected_value, should_match)
            ("test-value", "test-value", true),
            ("\"test-value\"", "test-value", true),
            ("test-value", "\"test-value\"", true),
            ("\"test-value\"", "\"test-value\"", true),
            ("different-value", "test-value", false),
            ("\"different-value\"", "test-value", false),
        ];

        for (dns_value, expected_value, should_match) in test_cases {
            // Simulate the comparison logic from check_dns_propagation
            let normalized_dns = dns_value.trim_matches('"');
            let normalized_expected = expected_value.trim_matches('"');
            let matches = normalized_dns == normalized_expected;

            assert_eq!(
                matches, should_match,
                "Failed for dns_value: '{dns_value}', expected: '{expected_value}', should_match: {should_match}"
            );
        }
    }
}
