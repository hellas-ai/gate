use crate::bootstrap::BootstrapTokenManager;
use crate::config::Settings;
use gate_core::InferenceBackend;
use gate_http::services::{AuthService, JwtService, WebAuthnService};
use std::sync::Arc;

/// Application state containing plugin manager and services
#[derive(Clone)]
pub struct ServerState {
    pub auth_service: Arc<AuthService>,
    pub webauthn_service: Arc<WebAuthnService>,
    pub jwt_service: Arc<JwtService>,
    pub settings: Arc<Settings>,
    pub bootstrap_manager: Arc<BootstrapTokenManager>,
    pub inference_service: Option<Arc<dyn InferenceBackend>>,
}

// Implement AsRef for each service Arc to allow easy access
impl AsRef<Arc<AuthService>> for ServerState {
    fn as_ref(&self) -> &Arc<AuthService> {
        &self.auth_service
    }
}

impl AsRef<Arc<WebAuthnService>> for ServerState {
    fn as_ref(&self) -> &Arc<WebAuthnService> {
        &self.webauthn_service
    }
}

impl AsRef<Arc<JwtService>> for ServerState {
    fn as_ref(&self) -> &Arc<JwtService> {
        &self.jwt_service
    }
}

impl AsRef<Arc<Settings>> for ServerState {
    fn as_ref(&self) -> &Arc<Settings> {
        &self.settings
    }
}

impl AsRef<Arc<BootstrapTokenManager>> for ServerState {
    fn as_ref(&self) -> &Arc<BootstrapTokenManager> {
        &self.bootstrap_manager
    }
}
