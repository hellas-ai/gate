//! Authentication API service

use crate::client::{get_client, set_auth_token};
use gate_http::types::{
    AuthCompleteRequest, AuthCompleteResponse, AuthStartResponse, RegisterCompleteRequest,
    RegisterCompleteResponse, RegisterStartResponse,
};
use serde::{Deserialize, Serialize};

// Define UserResponse locally since we can't access the typed client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserResponse {
    pub id: String,
    pub name: String,
    pub role: String,
    pub created_at: String,
    pub updated_at: String,
}
/// Authentication API service
#[derive(Clone)]
pub struct AuthApiService;

impl AuthApiService {
    /// Create a new auth API service
    pub fn new() -> Self {
        Self
    }

    /// Start registration process
    pub async fn start_registration(&self, name: String) -> Result<RegisterStartResponse, String> {
        let client = get_client().map_err(|e| format!("Failed to get client: {e}"))?;

        client.register_start(name).await.map_err(|e| e.to_string())
    }

    /// Complete registration with the credential
    pub async fn complete_registration(
        &self,
        session_id: String,
        credential: serde_json::Value,
        device_name: Option<String>,
        bootstrap_token: Option<String>,
    ) -> Result<RegisterCompleteResponse, String> {
        let client = get_client().map_err(|e| format!("Failed to get client: {e}"))?;

        let request = RegisterCompleteRequest {
            session_id,
            credential,
            device_name,
            bootstrap_token,
        };

        client
            .register_complete(request)
            .await
            .map_err(|e| e.to_string())
    }

    /// Start authentication process
    pub async fn start_authentication(&self) -> Result<AuthStartResponse, String> {
        let client = get_client().map_err(|e| format!("Failed to get client: {e}"))?;

        client.auth_start().await.map_err(|e| e.to_string())
    }

    /// Complete authentication with the credential
    pub async fn complete_authentication(
        &self,
        session_id: String,
        credential: serde_json::Value,
    ) -> Result<AuthCompleteResponse, String> {
        let client = get_client().map_err(|e| format!("Failed to get client: {e}"))?;

        let request = AuthCompleteRequest {
            session_id,
            credential,
        };

        client
            .auth_complete(request)
            .await
            .map_err(|e| e.to_string())
    }

    /// Get current user info
    pub async fn get_current_user(&self, token: &str) -> Result<UserResponse, String> {
        // Update the client with the token
        set_auth_token(Some(token)).map_err(|e| format!("Failed to set auth token: {e}"))?;

        // Get the authenticated client
        let client = get_client().map_err(|e| format!("Failed to get client: {e}"))?;

        // Make the API call
        let response = client
            .request(reqwest::Method::GET, "/api/auth/me")
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        if !response.status().is_success() {
            return Err(format!("Failed to get user info: {}", response.status()));
        }

        response
            .json::<UserResponse>()
            .await
            .map_err(|e| format!("Failed to parse response: {e}"))
    }
}
