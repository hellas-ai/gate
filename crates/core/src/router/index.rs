//! ConnectorIndex: caller-managed snapshots of connector descriptions and health

use tokio::sync::RwLock;

use super::connector::ConnectorDescription;
use super::registry::ConnectorRegistry;
use super::types::ConnectorHealth;
use std::collections::HashMap;
use std::sync::Arc;

/// Snapshot of a connector's description and health
#[derive(Clone, Debug)]
pub struct ConnectorSnapshot {
    pub description: ConnectorDescription,
    pub health: ConnectorHealth,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Caller-managed index of connector snapshots for fast routing
#[derive(Default, Debug, Clone)]
pub struct ConnectorIndex {
    inner: Arc<RwLock<HashMap<String, ConnectorSnapshot>>>,
}

impl ConnectorIndex {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set or update a snapshot for a connector ID (URL)
    pub async fn set_snapshot(
        &self,
        connector_id: String,
        description: ConnectorDescription,
        health: ConnectorHealth,
    ) {
        let mut guard = self.inner.write().await;
        guard.insert(
            connector_id,
            ConnectorSnapshot {
                description,
                health,
                updated_at: chrono::Utc::now(),
            },
        );
    }

    /// Remove a snapshot
    pub async fn remove(&self, connector_id: &str) {
        let mut guard = self.inner.write().await;
        guard.remove(connector_id);
    }

    /// Get a snapshot by connector ID
    pub async fn get(&self, connector_id: &str) -> Option<ConnectorSnapshot> {
        let guard = self.inner.read().await;
        guard.get(connector_id).cloned()
    }

    /// List all snapshots
    pub async fn list(&self) -> Vec<(String, ConnectorSnapshot)> {
        let guard = self.inner.read().await;
        guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Refresh snapshots from the given registry (all connectors)
    pub async fn refresh_from_registry(&self, registry: &ConnectorRegistry) -> usize {
        let ids = registry.list_ids().await;
        self.refresh_subset_from_registry(registry, &ids).await
    }

    /// Refresh a subset of connector IDs from the registry
    pub async fn refresh_subset_from_registry(
        &self,
        registry: &ConnectorRegistry,
        ids: &[String],
    ) -> usize {
        let mut count = 0usize;
        for id in ids {
            if let Some(connector) = registry.get(id).await {
                let desc = connector.describe().await;
                let health = connector.probe().await;
                self.set_snapshot(id.clone(), desc, health).await;
                count += 1;
            }
        }
        count
    }
}
