//! WebAuthn service for managing WebAuthn operations

use crate::error::HttpError;
use crate::middleware::webauthn::{WebAuthnConfig, WebAuthnSession, WebAuthnState};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use gate_core::WebAuthnBackend;
use std::sync::Arc;
use uuid::Uuid;
use webauthn_rs::prelude::*;

/// WebAuthn service for authentication operations
pub struct WebAuthnService {
    state: Arc<WebAuthnState>,
    backend: Arc<dyn WebAuthnBackend>,
}

impl WebAuthnService {
    /// Create a new WebAuthn service
    pub fn new(
        config: WebAuthnConfig,
        backend: Arc<dyn WebAuthnBackend>,
    ) -> Result<Self, HttpError> {
        let state = WebAuthnState::new(config).map_err(|e| {
            HttpError::InternalServerError(format!("Failed to initialize WebAuthn: {e}"))
        })?;

        Ok(Self {
            state: Arc::new(state),
            backend,
        })
    }

    /// Get the WebAuthn state
    pub fn state(&self) -> Arc<WebAuthnState> {
        self.state.clone()
    }

    /// Start registration for a new user
    pub async fn start_registration(
        &self,
        user_name: String,
    ) -> Result<(serde_json::Value, String), HttpError> {
        let user_id = Uuid::new_v4();

        // Start passkey registration
        let (ccr, reg_state) = self
            .state
            .webauthn()
            .read()
            .await
            .start_passkey_registration(user_id, &user_name, &user_name, None)
            .map_err(|e| {
                HttpError::InternalServerError(format!("Failed to start registration: {e}"))
            })?;

        // Generate session ID
        let session_id = WebAuthnState::generate_session_id();

        // Store session
        let session = WebAuthnSession {
            user_name: Some(user_name),
            registration_state: Some(reg_state),
            authentication_state: None,
            created_at: Utc::now(),
        };
        self.state.store_session(session_id.clone(), session).await;

        // Convert to JSON
        let challenge_json = serde_json::to_value(ccr).map_err(|e| {
            HttpError::InternalServerError(format!("Failed to serialize challenge: {e}"))
        })?;

        Ok((challenge_json, session_id))
    }

    /// Complete registration with credential
    pub async fn complete_registration(
        &self,
        session_id: String,
        credential_json: serde_json::Value,
    ) -> Result<(Passkey, String, String), HttpError> {
        // Get session
        let session = self
            .state
            .get_session(&session_id)
            .await
            .ok_or_else(|| HttpError::BadRequest("Invalid session".to_string()))?;

        let reg_state = session
            .registration_state
            .ok_or_else(|| HttpError::BadRequest("Invalid session state".to_string()))?;

        let user_name = session
            .user_name
            .ok_or_else(|| HttpError::BadRequest("Missing user name".to_string()))?;

        // Deserialize the credential
        let credential: RegisterPublicKeyCredential = serde_json::from_value(credential_json)
            .map_err(|e| HttpError::BadRequest(format!("Invalid credential format: {e}")))?;

        // Complete registration
        let passkey = self
            .state
            .webauthn()
            .read()
            .await
            .finish_passkey_registration(&credential, &reg_state)
            .map_err(|e| {
                // Provide user-friendly error messages for common WebAuthn errors
                let error_msg = e.to_string();
                if error_msg.contains("origin does not match") {
                    HttpError::BadRequest(
                        "Security error: The domain you're accessing from doesn't match the expected domain. \
                        Please ensure you're accessing from the correct URL.".to_string()
                    )
                } else {
                    HttpError::BadRequest(format!("Registration failed: {e}"))
                }
            })?;

        // Get credential ID
        let credential_id = URL_SAFE_NO_PAD.encode(passkey.cred_id());

        // Clean up session
        self.state.remove_session(&session_id).await;

        Ok((passkey, credential_id, user_name))
    }

    /// Start authentication
    pub async fn start_authentication(&self) -> Result<(serde_json::Value, String), HttpError> {
        // Get all credentials
        let credentials = self.backend.list_all_credentials().await.map_err(|e| {
            HttpError::InternalServerError(format!("Failed to list credentials: {e}"))
        })?;

        if credentials.is_empty() {
            return Err(HttpError::NotFound("No credentials registered".to_string()));
        }

        // Convert stored credentials back to passkeys
        let mut passkeys: Vec<Passkey> = Vec::new();
        for cred in credentials {
            match serde_json::from_slice::<Passkey>(&cred.public_key) {
                Ok(passkey) => passkeys.push(passkey),
                Err(e) => {
                    // Log error but continue with other credentials
                    tracing::warn!(
                        "Failed to deserialize passkey for credential {}: {}",
                        cred.credential_id,
                        e
                    );
                }
            }
        }

        if passkeys.is_empty() {
            return Err(HttpError::InternalServerError(
                "No valid passkeys found in storage".to_string(),
            ));
        }

        // Start authentication
        let (rcr, auth_state) = self
            .state
            .webauthn()
            .read()
            .await
            .start_passkey_authentication(&passkeys)
            .map_err(|e| {
                HttpError::InternalServerError(format!("Failed to start authentication: {e}"))
            })?;

        // Generate session ID
        let session_id = WebAuthnState::generate_session_id();

        // Store session
        let session = WebAuthnSession {
            user_name: None,
            registration_state: None,
            authentication_state: Some(auth_state),
            created_at: Utc::now(),
        };
        self.state.store_session(session_id.clone(), session).await;

        // Convert to JSON
        let challenge_json = serde_json::to_value(rcr).map_err(|e| {
            HttpError::InternalServerError(format!("Failed to serialize challenge: {e}"))
        })?;

        Ok((challenge_json, session_id))
    }

    /// Complete authentication
    pub async fn complete_authentication(
        &self,
        session_id: String,
        credential_json: serde_json::Value,
    ) -> Result<(String, u32), HttpError> {
        // Get session
        let session = self
            .state
            .get_session(&session_id)
            .await
            .ok_or_else(|| HttpError::BadRequest("Invalid session".to_string()))?;

        let auth_state = session
            .authentication_state
            .ok_or_else(|| HttpError::BadRequest("Invalid session state".to_string()))?;

        // Deserialize the credential
        let credential: PublicKeyCredential = serde_json::from_value(credential_json)
            .map_err(|e| HttpError::BadRequest(format!("Invalid credential format: {e}")))?;

        // Complete authentication
        let auth_result = self
            .state
            .webauthn()
            .read()
            .await
            .finish_passkey_authentication(&credential, &auth_state)
            .map_err(|e| HttpError::AuthenticationFailed(format!("Invalid credential: {e}")))?;

        // Get credential ID
        let credential_id = URL_SAFE_NO_PAD.encode(auth_result.cred_id());

        // Clean up session
        self.state.remove_session(&session_id).await;

        Ok((credential_id, auth_result.counter()))
    }

    /// Cleanup expired sessions periodically
    pub async fn cleanup_sessions(&self) {
        self.state.cleanup_expired_sessions().await;
    }

    /// Add a new allowed origin dynamically
    pub async fn add_allowed_origin(&self, origin: String) -> Result<(), HttpError> {
        self.state
            .add_allowed_origin(origin)
            .await
            .map_err(|e| HttpError::InternalServerError(format!("Failed to add origin: {e}")))
    }
}
