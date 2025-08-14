//! Custom authentication routes with registration control

use crate::bootstrap::BootstrapTokenManager;
use crate::config::Settings;
use crate::permissions::LocalPermissionManager;
use axum::{extract::State, response::Json};
use chrono::{DateTime, Utc};
use gate_core::{BootstrapTokenValidator, User};
use gate_http::{
    error::HttpError,
    services::{AuthService, HttpIdentity, WebAuthnService},
    state::AppState,
    types::{RegisterCompleteRequest, RegisterCompleteResponse},
};
use std::sync::Arc;
use tracing::{info, instrument, warn};
use utoipa_axum::{router::OpenApiRouter, routes};

/// Check bootstrap status
#[utoipa::path(
    get,
    path = "/auth/bootstrap/status",
    responses(
        (status = 200, description = "Bootstrap status", body = serde_json::Value),
        (status = 500, description = "Internal server error"),
    ),
    tag = "authentication"
)]
#[instrument(name = "get_bootstrap_status", skip(app_state))]
pub async fn get_bootstrap_status<T>(
    State(app_state): State<AppState<T>>,
) -> Result<Json<serde_json::Value>, HttpError>
where
    T: Clone + Send + Sync + 'static + AsRef<Arc<BootstrapTokenManager>>,
{
    let bootstrap_manager: &Arc<BootstrapTokenManager> = app_state.data.as_ref().as_ref();

    let needs_bootstrap = bootstrap_manager.needs_bootstrap().await.map_err(|e| {
        HttpError::InternalServerError(format!("Failed to check bootstrap status: {e}"))
    })?;

    let is_complete = bootstrap_manager.is_bootstrap_complete().await;

    Ok(Json(serde_json::json!({
        "needs_bootstrap": needs_bootstrap,
        "is_complete": is_complete,
        "message": if needs_bootstrap {
            "System requires initial admin user setup"
        } else {
            "System is bootstrapped"
        }
    })))
}

#[derive(serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct CurrentUser {
    pub id: String,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<User> for CurrentUser {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            name: user.name,
            created_at: user.created_at,
            updated_at: user.updated_at,
        }
    }
}

/// Complete WebAuthn registration with bootstrap token validation
/// This endpoint is specifically for the first-time setup with a bootstrap token
#[utoipa::path(
    post,
    path = "/auth/webauthn/register/bootstrap",
    request_body = RegisterCompleteRequest,
    responses(
        (status = 200, description = "Bootstrap registration completed", body = RegisterCompleteResponse),
        (status = 400, description = "Bad request"),
        (status = 403, description = "Invalid or missing bootstrap token"),
        (status = 500, description = "Internal server error"),
    ),
    tag = "authentication"
)]
#[instrument(
    name = "bootstrap_register",
    skip(app_state, request),
    fields(
        session_id = %request.session_id,
        device_name = ?request.device_name
    )
)]
pub async fn register_with_bootstrap<T>(
    State(app_state): State<AppState<T>>,
    Json(request): Json<RegisterCompleteRequest>,
) -> Result<Json<RegisterCompleteResponse>, HttpError>
where
    T: Clone
        + Send
        + Sync
        + 'static
        + AsRef<Option<Arc<WebAuthnService>>>
        + AsRef<Arc<AuthService>>
        + AsRef<Arc<BootstrapTokenManager>>
        + AsRef<Arc<LocalPermissionManager>>,
{
    let maybe_webauthn_service: &Option<Arc<WebAuthnService>> = app_state.data.as_ref().as_ref();
    let webauthn_service = maybe_webauthn_service
        .as_ref()
        .expect("WebAuthn service required");
    let auth_service: &Arc<AuthService> = app_state.data.as_ref().as_ref();
    let bootstrap_manager: &Arc<BootstrapTokenManager> = app_state.data.as_ref().as_ref();
    let permission_manager: &Arc<LocalPermissionManager> = app_state.data.as_ref().as_ref();

    // Bootstrap token is required for this endpoint
    let token = request.bootstrap_token.as_ref().ok_or_else(|| {
        HttpError::AuthorizationFailed("Bootstrap token is required for this endpoint".to_string())
    })?;

    info!("Bootstrap registration attempt with token: {}", token);

    // Check if bootstrap is still needed
    let needs_bootstrap = bootstrap_manager.needs_bootstrap().await.unwrap_or(true);
    info!("Bootstrap needed: {}", needs_bootstrap);

    if !needs_bootstrap {
        return Err(HttpError::AuthorizationFailed(
            "Bootstrap already completed".to_string(),
        ));
    }

    // Validate the bootstrap token
    let token_valid = bootstrap_manager
        .validate_token(token)
        .await
        .unwrap_or(false);
    info!("Bootstrap token validation result: {}", token_valid);

    if !token_valid {
        warn!("Invalid bootstrap token provided during registration");
        return Err(HttpError::AuthorizationFailed(
            "Invalid bootstrap token".to_string(),
        ));
    }

    // Complete the WebAuthn registration
    let device_name = request.device_name.clone();
    let (passkey, credential_id, user_name) = webauthn_service
        .complete_registration(request.session_id.clone(), request.credential.clone())
        .await?;

    let user = User {
        id: credential_id.clone(),
        name: Some(user_name.clone()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        disabled_at: None,
        metadata: std::collections::HashMap::new(),
    };

    // Register the user using the common auth service
    let response = auth_service
        .complete_registration(user, credential_id.clone(), device_name, passkey)
        .await?;

    // Grant admin permissions to the bootstrap user
    info!("Granting admin permissions to user: {}", credential_id);
    permission_manager
        .initialize_owner(&credential_id)
        .await
        .map_err(|e| {
            HttpError::InternalServerError(format!("Failed to grant admin permissions: {e}"))
        })?;
    info!("Admin permissions granted successfully");

    // Mark bootstrap token as used
    bootstrap_manager.mark_as_used().await.map_err(|e| {
        HttpError::InternalServerError(format!("Failed to mark bootstrap token as used: {e}"))
    })?;

    info!(
        "Bootstrap user registered with admin permissions: {} ({})",
        response.name, response.user_id
    );

    Ok(Json(response))
}

/// Get current user information
#[utoipa::path(
    get,
    path = "/api/auth/me",
    operation_id = "get_current_user",
    description = "Get current user information",
    responses(
        (status = 200, description = "User information", body = CurrentUser),
        (status = 401, description = "Unauthorized")
    ),
    security(
        ("BearerAuth" = [])
    )
)]
async fn get_current_user<T>(
    State(app_state): State<AppState<T>>,
    identity: HttpIdentity,
) -> Result<Json<CurrentUser>, HttpError>
where
    T: AsRef<Option<Arc<WebAuthnService>>>
        + AsRef<Arc<AuthService>>
        + AsRef<Arc<Settings>>
        + AsRef<Arc<BootstrapTokenManager>>,
{
    // Get user from database
    let user_data = app_state
        .state_backend
        .get_user(&identity.id)
        .await
        .map_err(|e| HttpError::InternalServerError(format!("Failed to get user: {e}")))?
        .ok_or_else(|| HttpError::AuthorizationFailed("User not found".to_string()))?;

    Ok(Json(user_data.into()))
}

/// Add custom auth routes
pub fn add_routes<T>(router: OpenApiRouter<AppState<T>>) -> OpenApiRouter<AppState<T>>
where
    T: Clone
        + Send
        + Sync
        + 'static
        + AsRef<Option<Arc<WebAuthnService>>>
        + AsRef<Arc<AuthService>>
        + AsRef<Arc<Settings>>
        + AsRef<Arc<BootstrapTokenManager>>
        + AsRef<Arc<LocalPermissionManager>>,
{
    router
        .routes(routes!(get_bootstrap_status))
        .routes(routes!(get_current_user))
        .routes(routes!(register_with_bootstrap))
}
