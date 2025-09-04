use super::sink::Sink;
use std::collections::HashMap;
use std::sync::Arc;

/// Registry for managing sinks
pub struct SinkRegistry {
    sinks: Arc<tokio::sync::RwLock<HashMap<String, Arc<dyn Sink>>>>,
}

impl SinkRegistry {
    pub fn new() -> Self {
        Self {
            sinks: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }
    pub async fn register(&self, id: String, sink: Arc<dyn Sink>) {
        let mut sinks = self.sinks.write().await;
        sinks.insert(id, sink);
    }
    pub async fn get(&self, id: &str) -> Option<Arc<dyn Sink>> {
        let sinks = self.sinks.read().await;
        sinks.get(id).cloned()
    }
    pub async fn remove(&self, id: &str) {
        let mut sinks = self.sinks.write().await;
        sinks.remove(id);
    }
    pub async fn list_ids(&self) -> Vec<String> {
        let sinks = self.sinks.read().await;
        sinks.keys().cloned().collect()
    }
    pub async fn get_all(&self) -> Vec<Arc<dyn Sink>> {
        let sinks = self.sinks.read().await;
        sinks.values().cloned().collect()
    }
}

impl Default for SinkRegistry {
    fn default() -> Self {
        Self::new()
    }
}
