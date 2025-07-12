//! Application state management

use crate::forwarding::UpstreamRegistry;
#[cfg(not(target_arch = "wasm32"))]
use crate::middleware::webauthn::WebAuthnState;
use gate_core::{InferenceBackend, RequestContext, StateBackend, WebAuthnBackend};
use std::sync::Arc;

/// Shared application state
///
/// This struct holds the shared state that can be accessed by all handlers
/// and middleware in the application. It's designed to be extensible through
/// the use of a generic type parameter.
#[derive(Clone)]
pub struct AppState<T = ()> {
    /// Request context for platform-specific operations
    pub context: Arc<dyn RequestContext>,
    /// State backend for data persistence
    pub state_backend: Arc<dyn StateBackend>,
    /// WebAuthn backend for credential storage
    pub webauthn_backend: Option<Arc<dyn WebAuthnBackend>>,
    /// WebAuthn state manager
    #[cfg(not(target_arch = "wasm32"))]
    pub webauthn_state: Option<Arc<WebAuthnState>>,
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
        context: Arc<dyn RequestContext>,
        state_backend: Arc<dyn StateBackend>,
        data: T,
    ) -> Self {
        Self {
            context,
            state_backend,
            webauthn_backend: None,
            #[cfg(not(target_arch = "wasm32"))]
            webauthn_state: None,
            upstream_registry: Arc::new(UpstreamRegistry::new()),
            inference_backend: None,
            data: Arc::new(data),
        }
    }

    /// Create a new AppState with WebAuthn support
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_webauthn(
        context: Arc<dyn RequestContext>,
        state_backend: Arc<dyn StateBackend>,
        webauthn_backend: Arc<dyn WebAuthnBackend>,
        webauthn_state: Arc<WebAuthnState>,
        data: T,
    ) -> Self {
        Self {
            context,
            state_backend,
            webauthn_backend: Some(webauthn_backend),
            webauthn_state: Some(webauthn_state),
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

#[cfg(test)]
impl Default for AppState<()> {
    fn default() -> Self {
        use gate_core::tests::{context::MockRequestContext, state::InMemoryBackend};

        Self::new(
            Arc::new(MockRequestContext::default()),
            Arc::new(InMemoryBackend::default()),
            (),
        )
    }
}
