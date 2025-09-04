//! Authentication API client methods

use super::{ClientError, GateClient};
use crate::types::{
    AuthCompleteRequest, AuthCompleteResponse, AuthStartResponse, RegisterCompleteRequest,
    RegisterCompleteResponse, RegisterStartRequest, RegisterStartResponse,
};

impl GateClient {
    /// Start WebAuthn registration
    pub async fn register_start(&self, name: String) -> Result<RegisterStartResponse, ClientError> {
        let req = self
            .request(reqwest::Method::POST, "/auth/webauthn/register/start")?
            .json(&RegisterStartRequest { name });
        self.execute(req).await
    }

    /// Complete WebAuthn registration
    pub async fn register_complete(
        &self,
        request: RegisterCompleteRequest,
    ) -> Result<RegisterCompleteResponse, ClientError> {
        let req = self
            .request(reqwest::Method::POST, "/auth/webauthn/register/complete")?
            .json(&request);
        self.execute(req).await
    }

    /// Start WebAuthn authentication
    pub async fn auth_start(&self) -> Result<AuthStartResponse, ClientError> {
        let req = self.request(reqwest::Method::POST, "/auth/webauthn/authenticate/start")?;
        self.execute(req).await
    }

    /// Complete WebAuthn authentication
    pub async fn auth_complete(
        &self,
        request: AuthCompleteRequest,
    ) -> Result<AuthCompleteResponse, ClientError> {
        let req = self
            .request(
                reqwest::Method::POST,
                "/auth/webauthn/authenticate/complete",
            )?
            .json(&request);
        self.execute(req).await
    }
}
