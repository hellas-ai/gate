//! Authentication API service

use crate::client::create_public_client;
use gate_http::types::{
    AuthCompleteRequest, AuthCompleteResponse, AuthStartResponse, RegisterCompleteRequest,
    RegisterCompleteResponse, RegisterStartResponse,
};
/// Authentication API service
#[derive(Clone)]
pub struct AuthApiService;

impl AuthApiService {
    /// Create a new auth API service
    pub fn new() -> Self {
        Self
    }
}

impl Default for AuthApiService {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthApiService {
    /// Start registration process
    pub async fn start_registration(&self, name: String) -> Result<RegisterStartResponse, String> {
        let client = create_public_client().map_err(|e| format!("Failed to get client: {e}"))?;

        let request = gate_http::types::RegisterStartRequest { name };
        client
            .register_start(request)
            .await
            .map_err(|e| e.to_string())
    }

    /// Complete registration with the credential
    pub async fn complete_registration(
        &self,
        session_id: String,
        credential: serde_json::Value,
        device_name: Option<String>,
        bootstrap_token: Option<String>,
    ) -> Result<RegisterCompleteResponse, String> {
        let client = create_public_client().map_err(|e| format!("Failed to get client: {e}"))?;

        let request = RegisterCompleteRequest {
            session_id,
            credential,
            device_name,
            bootstrap_token: bootstrap_token.clone(),
        };

        // Use bootstrap endpoint if bootstrap token is present
        if bootstrap_token.is_some() {
            client
                .register_bootstrap(request)
                .await
                .map_err(|e| e.to_string())
        } else {
            client
                .register_complete(request)
                .await
                .map_err(|e| e.to_string())
        }
    }

    /// Start authentication process
    pub async fn start_authentication(&self) -> Result<AuthStartResponse, String> {
        let client = create_public_client().map_err(|e| format!("Failed to get client: {e}"))?;

        client.auth_start().await.map_err(|e| e.to_string())
    }

    /// Complete authentication with the credential
    pub async fn complete_authentication(
        &self,
        session_id: String,
        credential: serde_json::Value,
    ) -> Result<AuthCompleteResponse, String> {
        let client = create_public_client().map_err(|e| format!("Failed to get client: {e}"))?;

        let request = AuthCompleteRequest {
            session_id,
            credential,
        };

        client
            .auth_complete(request)
            .await
            .map_err(|e| e.to_string())
    }
}
