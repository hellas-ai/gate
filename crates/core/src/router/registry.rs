use super::connector::Connector;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry for managing connectors
pub struct ConnectorRegistry {
    connectors: Arc<tokio::sync::RwLock<HashMap<String, Arc<dyn Connector>>>>,
}

impl ConnectorRegistry {
    pub fn new() -> Self {
        Self {
            connectors: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }
    pub async fn register(&self, id: String, connector: Arc<dyn Connector>) {
        let mut connectors = self.connectors.write().await;
        connectors.insert(id, connector);
    }
    pub async fn get(&self, id: &str) -> Option<Arc<dyn Connector>> {
        let connectors = self.connectors.read().await;
        connectors.get(id).cloned()
    }
    pub async fn remove(&self, id: &str) {
        let mut connectors = self.connectors.write().await;
        connectors.remove(id);
    }
    pub async fn list_ids(&self) -> Vec<String> {
        let connectors = self.connectors.read().await;
        connectors.keys().cloned().collect()
    }
    pub async fn get_all(&self) -> Vec<Arc<dyn Connector>> {
        let connectors = self.connectors.read().await;
        connectors.values().cloned().collect()
    }
}

impl Default for ConnectorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
