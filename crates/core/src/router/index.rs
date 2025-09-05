//! SinkIndex: caller-managed snapshots of sink descriptions and health

use tokio::sync::RwLock;

use super::registry::SinkRegistry;
use super::sink::SinkDescription;
use super::types::SinkHealth;
use std::collections::HashMap;
use std::sync::Arc;

/// Snapshot of a sink's description and health
#[derive(Clone, Debug)]
pub struct SinkSnapshot {
    pub description: SinkDescription,
    pub health: SinkHealth,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Caller-managed index of sink snapshots for fast routing
#[derive(Default, Debug, Clone)]
pub struct SinkIndex {
    inner: Arc<RwLock<HashMap<String, SinkSnapshot>>>,
}

impl SinkIndex {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set or update a snapshot for a sink ID (URL)
    pub async fn set_snapshot(
        &self,
        sink_id: String,
        description: SinkDescription,
        health: SinkHealth,
    ) {
        let mut guard = self.inner.write().await;
        guard.insert(
            sink_id,
            SinkSnapshot {
                description,
                health,
                updated_at: chrono::Utc::now(),
            },
        );
    }

    /// Remove a snapshot
    pub async fn remove(&self, sink_id: &str) {
        let mut guard = self.inner.write().await;
        guard.remove(sink_id);
    }

    /// Get a snapshot by sink ID
    pub async fn get(&self, sink_id: &str) -> Option<SinkSnapshot> {
        let guard = self.inner.read().await;
        guard.get(sink_id).cloned()
    }

    /// List all snapshots
    pub async fn list(&self) -> Vec<(String, SinkSnapshot)> {
        let guard = self.inner.read().await;
        guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Refresh snapshots from the given registry (all sinks)
    pub async fn refresh_from_registry(&self, registry: &SinkRegistry) -> usize {
        let ids = registry.list_ids().await;
        self.refresh_subset_from_registry(registry, &ids).await
    }

    /// Refresh a subset of sink IDs from the registry
    pub async fn refresh_subset_from_registry(
        &self,
        registry: &SinkRegistry,
        ids: &[String],
    ) -> usize {
        let mut count = 0usize;
        for id in ids {
            if let Some(sink) = registry.get(id).await {
                let desc = sink.describe().await;
                let health = sink.probe().await;
                self.set_snapshot(id.clone(), desc, health).await;
                count += 1;
            }
        }
        count
    }
}
