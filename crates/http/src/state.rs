//! Application state management

use gate_core::StateBackend;
use gate_core::router::prelude::Router;
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
    /// Router for all routing decisions
    pub router: Option<Arc<Router>>,
    /// Custom state data
    pub data: Arc<T>,
}

impl<T> AppState<T> {
    /// Create a new AppState with the given components
    pub fn new(state_backend: Arc<dyn StateBackend>, data: T) -> Self {
        Self {
            state_backend,
            router: None,
            data: Arc::new(data),
        }
    }

    /// Set the router
    pub fn with_router(mut self, router: Arc<Router>) -> Self {
        self.router = Some(router);
        self
    }
}
