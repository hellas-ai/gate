use chrono::Utc;
use gate_core::{StateBackend, User};
use gate_http::error::HttpError;
use gate_http::services::{HttpContext, HttpIdentity, JwtService};
use gate_http::types::{AuthCompleteResponse, RegisterCompleteResponse};
use gate_sqlx::{SqliteWebAuthnBackend, StoredCredential};
use std::sync::Arc;
use webauthn_rs::prelude::*;

/// Authentication service that coordinates JWT and WebAuthn operations
pub struct AuthService {
    jwt_service: Arc<JwtService>,
    state_backend: Arc<dyn StateBackend>,
    webauthn_backend: Arc<SqliteWebAuthnBackend>,
}

impl AuthService {
    pub fn new(
        jwt_service: Arc<JwtService>,
        state_backend: Arc<dyn StateBackend>,
        webauthn_backend: Arc<SqliteWebAuthnBackend>,
    ) -> Self {
        Self {
            jwt_service,
            state_backend,
            webauthn_backend,
        }
    }

    pub async fn complete_registration(
        &self,
        user: User,
        credential_id: String,
        device_name: Option<String>,
        passkey: Passkey,
    ) -> Result<RegisterCompleteResponse, HttpError> {
        self.state_backend
            .create_user(&user)
            .await
            .map_err(|e| HttpError::InternalServerError(format!("Failed to create user: {e}")))?;

        let passkey_data = serde_json::to_vec(&passkey).map_err(|e| {
            HttpError::InternalServerError(format!("Failed to serialize passkey: {e}"))
        })?;

        let stored_credential = StoredCredential {
            credential_id: credential_id.clone(),
            user_id: user.id.clone(),
            public_key: passkey_data,
            aaguid: None,
            counter: 0,
            created_at: Utc::now(),
            last_used_at: None,
            device_name,
        };

        self.webauthn_backend
            .store_webauthn_credential(&stored_credential)
            .await
            .map_err(|e| {
                HttpError::InternalServerError(format!("Failed to store credential: {e}"))
            })?;

        let token = self
            .jwt_service
            .generate_token(&user.id, user.name.as_deref())?;

        Ok(RegisterCompleteResponse {
            user_id: user.id.clone(),
            name: user.name.unwrap_or_else(|| "User".to_string()),
            credential_id,
            token,
        })
    }

    pub async fn complete_authentication(
        &self,
        credential_id: String,
        counter: u32,
    ) -> Result<AuthCompleteResponse, HttpError> {
        let user = self
            .state_backend
            .get_user_by_id(&credential_id)
            .await
            .map_err(|e| HttpError::InternalServerError(format!("Failed to get user: {e}")))?
            .ok_or_else(|| HttpError::NotFound("User not found".to_string()))?;

        self.webauthn_backend
            .update_credential_counter(&credential_id, counter)
            .await
            .map_err(|e| {
                HttpError::InternalServerError(format!("Failed to update credential: {e}"))
            })?;

        let token = self
            .jwt_service
            .generate_token(&user.id, user.name.as_deref())?;

        Ok(AuthCompleteResponse {
            user_id: user.id,
            name: user.name.unwrap_or_else(|| "User".to_string()),
            token,
        })
    }

    pub fn validate_token(&self, token: &str) -> Result<HttpIdentity, HttpError> {
        let claims = self.jwt_service.validate_token(token)?;

        let identity = HttpIdentity::new(
            claims.sub.clone(),
            "jwt".to_string(),
            HttpContext::new()
                .with_attribute("auth_method", "webauthn")
                .with_attribute("issued_at", claims.iat.to_string())
                .with_attribute("expires_at", claims.exp.to_string())
                .with_attribute("name", claims.name.as_deref().unwrap_or("")),
        );

        Ok(identity)
    }

    pub fn authenticate_from_header(&self, auth_header: &str) -> Result<HttpIdentity, HttpError> {
        let token = self.jwt_service.extract_bearer_token(auth_header)?;
        self.validate_token(token)
    }
}
