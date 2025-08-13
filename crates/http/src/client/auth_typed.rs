//! Type-safe authentication client extensions

use super::{
    error::ClientError,
    typed::{AuthenticatedGateClient, PublicGateClient},
};
use crate::types::{
    AuthCompleteRequest, AuthCompleteResponse, AuthStartResponse, RegisterCompleteRequest,
    RegisterCompleteResponse, RegisterStartRequest, RegisterStartResponse,
};
use serde::{Deserialize, Serialize};

/// Authentication endpoints for public client
impl PublicGateClient {
    /// Start WebAuthn registration (public endpoint)
    pub async fn register_start(
        &self,
        request: RegisterStartRequest,
    ) -> Result<RegisterStartResponse, ClientError> {
        let req = self
            .request(reqwest::Method::POST, "/auth/webauthn/register/start")
            .json(&request);
        self.execute(req).await
    }

    /// Complete WebAuthn registration (public endpoint)
    pub async fn register_complete(
        &self,
        request: RegisterCompleteRequest,
    ) -> Result<RegisterCompleteResponse, ClientError> {
        let req = self
            .request(reqwest::Method::POST, "/auth/webauthn/register/complete")
            .json(&request);
        self.execute(req).await
    }

    /// Complete WebAuthn registration with bootstrap token (daemon-specific endpoint)
    pub async fn register_bootstrap(
        &self,
        request: RegisterCompleteRequest,
    ) -> Result<RegisterCompleteResponse, ClientError> {
        let req = self
            .request(reqwest::Method::POST, "/auth/webauthn/register/bootstrap")
            .json(&request);
        self.execute(req).await
    }

    /// Start WebAuthn authentication (public endpoint)
    pub async fn auth_start(&self) -> Result<AuthStartResponse, ClientError> {
        let req = self.request(reqwest::Method::POST, "/auth/webauthn/authenticate/start");
        self.execute(req).await
    }

    /// Complete WebAuthn authentication (public endpoint)
    pub async fn auth_complete(
        &self,
        request: AuthCompleteRequest,
    ) -> Result<AuthCompleteResponse, ClientError> {
        let req = self
            .request(
                reqwest::Method::POST,
                "/auth/webauthn/authenticate/complete",
            )
            .json(&request);
        self.execute(req).await
    }
}

/// Authentication endpoints for authenticated client
impl AuthenticatedGateClient {
    /// Get current user info (requires authentication)
    pub async fn get_me(&self) -> Result<UserResponse, ClientError> {
        let request = self.request(reqwest::Method::GET, "/api/auth/me");
        self.execute(request).await
    }
}

// Response types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserResponse {
    pub id: String,
    pub name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
