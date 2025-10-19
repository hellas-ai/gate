//! Authentication API service

use std::sync::Arc;

use gate_http::{
    client::{error::ClientError, PublicGateClient},
    types::{
        AuthCompleteRequest, AuthCompleteResponse, AuthStartResponse, RegisterCompleteResponse,
        RegisterStartResponse,
    },
};

/// Authentication API service
#[derive(Clone)]
pub struct AuthApiService {
    client: Arc<PublicGateClient>,
}

impl AuthApiService {
    pub fn new(client: PublicGateClient) -> Self {
        Self {
            client: Arc::new(client),
        }
    }

    /// Start registration process
    pub async fn start_registration(
        &self,
        name: &str,
    ) -> Result<RegisterStartResponse, ClientError> {
        self.client.register_start(name).await
    }

    /// Complete registration with the credential
    pub async fn complete_registration(
        &self,
        session_id: String,
        credential: serde_json::Value,
        device_name: Option<String>,
        bootstrap_token: Option<String>,
    ) -> Result<RegisterCompleteResponse, ClientError> {
        self.client
            .register_complete(&session_id, credential, device_name, bootstrap_token)
            .await
    }

    /// Start authentication process
    pub async fn start_authentication(&self) -> Result<AuthStartResponse, ClientError> {
        self.client.auth_start().await
    }

    /// Complete authentication with the credential
    pub async fn complete_authentication(
        &self,
        session_id: String,
        credential: serde_json::Value,
    ) -> Result<AuthCompleteResponse, ClientError> {
        let request = AuthCompleteRequest {
            session_id,
            credential,
        };
        self.client.auth_complete(request).await
    }
}
