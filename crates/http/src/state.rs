//! Application state management

use crate::forwarding::UpstreamRegistry;
use gate_core::{InferenceBackend, StateBackend};
use std::sync::Arc;

/// Shared application state
///
/// This struct holds the shared state that can be accessed by all handlers
/// and middleware in the application. It's designed to be extensible through
/// the use of a generic type parameter.
#[derive(Clone)]
pub struct AppState<T = ()> {
    /// State backend for data persistence
    pub state_backend: Arc<dyn StateBackend>,
    /// Registry for upstream providers
    pub upstream_registry: Arc<UpstreamRegistry>,
    /// Inference backend for local model inference
    pub inference_backend: Option<Arc<dyn InferenceBackend>>,
    /// Custom state data
    pub data: Arc<T>,
}

impl<T> AppState<T> {
    /// Create a new AppState with the given components
    pub fn new(
        // context: Arc<dyn RequestContext>,
        state_backend: Arc<dyn StateBackend>,
        data: T,
    ) -> Self {
        Self {
            // context,
            state_backend,
            upstream_registry: Arc::new(UpstreamRegistry::new()),
            inference_backend: None,
            data: Arc::new(data),
        }
    }

    /// Set the upstream registry
    pub fn with_upstream_registry(mut self, registry: Arc<UpstreamRegistry>) -> Self {
        self.upstream_registry = registry;
        self
    }

    /// Set the inference backend
    pub fn with_inference_backend(mut self, backend: Arc<dyn InferenceBackend>) -> Self {
        self.inference_backend = Some(backend);
        self
    }
}
