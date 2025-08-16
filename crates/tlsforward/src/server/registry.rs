//! Registry for mapping domains to P2P node addresses

use crate::common::error::{Result, TlsForwardError};
use gate_core::tracing::metrics::gauge;
use iroh::NodeId;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Entry in the proxy registry
#[derive(Debug, Clone)]
pub struct RegistryEntry {
    /// Full node ID
    pub node_id: NodeId,
    /// Optional metadata
    pub metadata: HashMap<String, String>,
    /// When the node connected
    pub connected_at: Instant,
    /// Last ping timestamp
    pub last_ping: Instant,
    /// Latest measured latency in milliseconds
    pub latency_ms: Option<u64>,
}

/// Registry for mapping short domain hashes to node information
#[derive(Clone)]
pub struct ProxyRegistry {
    /// Internal storage for mappings
    /// Key: short hash (first 16 chars of node ID hex)
    /// Value: registry entry
    entries: Arc<RwLock<HashMap<String, RegistryEntry>>>,
    /// Domain suffix (e.g., "private.hellas.ai")
    domain_suffix: String,
}

impl ProxyRegistry {
    /// Create a new proxy registry
    pub fn new(domain_suffix: String) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            domain_suffix,
        }
    }

    /// Register a node with its address
    pub async fn register(&self, node_id: NodeId) -> Result<String> {
        let short_hash = node_id.fmt_short();
        let now = Instant::now();
        let entry = RegistryEntry {
            node_id,
            metadata: HashMap::new(),
            connected_at: now,
            last_ping: now,
            latency_ms: None,
        };

        let mut entries = self.entries.write().await;
        entries.insert(short_hash.clone(), entry);
        gauge("relay_registry_nodes").set(entries.len() as i64);

        Ok(format!("{}.{}", short_hash, self.domain_suffix))
    }

    /// Unregister a node
    pub async fn unregister(&self, node_id: &NodeId) -> Result<()> {
        let short_hash = node_id.fmt_short();
        let mut entries = self.entries.write().await;
        entries.remove(&short_hash);
        gauge("relay_registry_nodes").set(entries.len() as i64);
        Ok(())
    }

    /// Look up a node by domain name
    pub async fn lookup(&self, domain: &str) -> Result<RegistryEntry> {
        // Extract short hash from domain
        let short_hash = self.extract_short_hash(domain)?;

        let entries = self.entries.read().await;
        debug!(
            "Looking up domain {} (short_hash: {}), registry has {} entries",
            domain,
            short_hash,
            entries.len()
        );
        entries.get(&short_hash).cloned().ok_or_else(|| {
            debug!("Node not found for short_hash: {}", short_hash);
            TlsForwardError::NodeNotFound(domain.to_string())
        })
    }

    /// Look up a node by short hash directly
    pub async fn lookup_by_hash(&self, short_hash: &str) -> Result<RegistryEntry> {
        let entries = self.entries.read().await;
        entries
            .get(short_hash)
            .cloned()
            .ok_or_else(|| TlsForwardError::NodeNotFound(short_hash.to_string()))
    }

    /// Get all registered nodes
    pub async fn list_all(&self) -> Vec<(String, RegistryEntry)> {
        let entries = self.entries.read().await;
        entries
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Update ping information for a node
    pub async fn update_ping(&self, node_id: &NodeId, latency_ms: Option<u64>) -> Result<()> {
        let short_hash = node_id.fmt_short();
        let mut entries = self.entries.write().await;

        debug!(
            "Updating ping for node {} (short_hash: {}), registry has {} entries",
            node_id,
            short_hash,
            entries.len()
        );

        if let Some(entry) = entries.get_mut(&short_hash) {
            entry.last_ping = Instant::now();
            if let Some(latency) = latency_ms {
                entry.latency_ms = Some(latency);
            }
            debug!("Successfully updated ping for node {}", node_id);
            Ok(())
        } else {
            debug!(
                "Failed to update ping - node {} (short_hash: {}) not found in registry",
                node_id, short_hash
            );
            Err(TlsForwardError::NodeNotFound(short_hash))
        }
    }

    /// Extract short hash from a full domain name
    fn extract_short_hash(&self, domain: &str) -> Result<String> {
        // Remove the domain suffix if present
        let domain = domain.trim_end_matches('.');
        let suffix = format!(".{}", self.domain_suffix);

        if let Some(prefix) = domain.strip_suffix(&suffix) {
            // Validate the short hash format (10 hex chars from fmt_short)
            if prefix.len() == 10 && prefix.chars().all(|c| c.is_ascii_hexdigit()) {
                Ok(prefix.to_string())
            } else {
                Err(TlsForwardError::InvalidSni(format!(
                    "Invalid short hash in domain: {domain}"
                )))
            }
        } else {
            Err(TlsForwardError::InvalidSni(format!(
                "Domain {} does not match suffix {}",
                domain, self.domain_suffix
            )))
        }
    }

    /// Look up a node by its ID
    pub async fn lookup_by_node_id(&self, node_id: &NodeId) -> Result<RegistryEntry> {
        let short_hash = node_id.fmt_short();
        self.lookup_by_hash(&short_hash).await
    }

    /// Clear all entries
    pub async fn clear(&self) {
        let mut entries = self.entries.write().await;
        entries.clear();
    }

    /// Get the number of registered nodes
    pub async fn len(&self) -> usize {
        let entries = self.entries.read().await;
        entries.len()
    }

    /// Check if the registry is empty
    pub async fn is_empty(&self) -> bool {
        let entries = self.entries.read().await;
        entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_hash_extraction() {
        let registry = ProxyRegistry::new("private.hellas.ai".to_string());

        // Test domain parsing (10 hex chars)
        assert!(
            registry
                .extract_short_hash("1234567890.private.hellas.ai")
                .is_ok()
        );
        assert!(
            registry
                .extract_short_hash("invalid.private.hellas.ai")
                .is_err()
        );
        assert!(
            registry
                .extract_short_hash("1234567890.wrong.domain")
                .is_err()
        );
    }
}
