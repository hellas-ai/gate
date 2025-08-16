//! Custom authentication routes with registration control

use crate::types::BootstrapStatusResponse;
use axum::{extract::State, response::Json};
use chrono::{DateTime, Utc};
use gate_core::User;
use gate_http::{
    error::HttpError,
    services::HttpIdentity,
    types::{
        AuthCompleteRequest, AuthCompleteResponse, AuthStartResponse, RegisterCompleteRequest,
        RegisterCompleteResponse, RegisterStartRequest, RegisterStartResponse,
    },
};
use utoipa_axum::{router::OpenApiRouter, routes};

/// Check bootstrap status
#[utoipa::path(
    get,
    path = "/auth/bootstrap/status",
    responses(
        (status = 200, description = "Bootstrap status", body = BootstrapStatusResponse),
        (status = 500, description = "Internal server error"),
    ),
    tag = "authentication"
)]
#[instrument(name = "get_bootstrap_status", skip(state))]
pub async fn get_bootstrap_status(
    State(state): State<gate_http::AppState<crate::MinimalState>>,
) -> Result<Json<BootstrapStatusResponse>, HttpError> {
    let bootstrap_manager = state
        .data
        .daemon
        .get_bootstrap_manager()
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;

    let needs_bootstrap = bootstrap_manager.needs_bootstrap().await.map_err(|e| {
        HttpError::InternalServerError(format!("Failed to check bootstrap status: {e}"))
    })?;

    let is_complete = bootstrap_manager.is_bootstrap_complete().await;

    Ok(Json(BootstrapStatusResponse {
        needs_bootstrap,
        is_complete,
        message: if needs_bootstrap {
            "System requires initial admin user setup".to_string()
        } else {
            "System is bootstrapped".to_string()
        },
    }))
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
    skip(state, request),
    fields(
        session_id = %request.session_id,
        device_name = ?request.device_name
    )
)]
pub async fn register_with_bootstrap(
    State(state): State<gate_http::AppState<crate::MinimalState>>,
    Json(request): Json<RegisterCompleteRequest>,
) -> Result<Json<RegisterCompleteResponse>, HttpError> {
    // Get services from daemon
    let webauthn_service = state
        .data
        .daemon
        .get_webauthn_service()
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?
        .ok_or_else(|| HttpError::BadRequest("WebAuthn service not enabled".to_string()))?;
    let bootstrap_manager = state
        .data
        .daemon
        .get_bootstrap_manager()
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;
    let permission_manager = state
        .data
        .daemon
        .get_permission_manager()
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;

    // Auth service is available directly from state for performance
    let auth_service = &state.data.auth_service;

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

/// Start WebAuthn registration
#[utoipa::path(
    post,
    path = "/auth/webauthn/register/start",
    request_body = RegisterStartRequest,
    responses(
        (status = 200, description = "Registration started", body = RegisterStartResponse),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error"),
    ),
    tag = "authentication"
)]
#[instrument(
    name = "webauthn_register_start",
    skip(state),
    fields(
        user_name = %request.name
    )
)]
pub async fn register_start(
    State(state): State<gate_http::AppState<crate::MinimalState>>,
    Json(request): Json<RegisterStartRequest>,
) -> Result<Json<RegisterStartResponse>, HttpError> {
    let webauthn_service = state
        .data
        .daemon
        .get_webauthn_service()
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?
        .ok_or_else(|| HttpError::BadRequest("WebAuthn service not enabled".to_string()))?;

    let (challenge, session_id) = webauthn_service.start_registration(request.name).await?;

    Ok(Json(RegisterStartResponse {
        challenge,
        session_id,
    }))
}

/// Complete WebAuthn registration
#[utoipa::path(
    post,
    path = "/auth/webauthn/register/complete",
    request_body = RegisterCompleteRequest,
    responses(
        (status = 200, description = "Registration completed", body = RegisterCompleteResponse),
        (status = 400, description = "Bad request"),
        (status = 403, description = "Registration not allowed"),
        (status = 500, description = "Internal server error"),
    ),
    tag = "authentication"
)]
#[instrument(
    name = "webauthn_register_complete",
    skip(state, request),
    fields(
        session_id = %request.session_id,
        device_name = ?request.device_name,
        has_bootstrap_token = request.bootstrap_token.is_some()
    )
)]
pub async fn register_complete(
    State(state): State<gate_http::AppState<crate::MinimalState>>,
    Json(request): Json<RegisterCompleteRequest>,
) -> Result<Json<RegisterCompleteResponse>, HttpError> {
    let webauthn_service = state
        .data
        .daemon
        .get_webauthn_service()
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?
        .ok_or_else(|| HttpError::BadRequest("WebAuthn service not enabled".to_string()))?;

    let auth_service = &state.data.auth_service;

    let device_name = request.device_name.clone();
    let (passkey, credential_id, user_name) = webauthn_service
        .complete_registration(request.session_id, request.credential)
        .await?;

    let user = User {
        id: credential_id.clone(),
        name: Some(user_name.clone()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        disabled_at: None,
        metadata: std::collections::HashMap::new(),
    };

    let response = auth_service
        .complete_registration(user, credential_id.clone(), device_name, passkey)
        .await?;

    info!("User registered: {} ({})", response.name, response.user_id);

    Ok(Json(response))
}

/// Start WebAuthn authentication
#[utoipa::path(
    post,
    path = "/auth/webauthn/authenticate/start",
    responses(
        (status = 200, description = "Authentication started", body = AuthStartResponse),
        (status = 404, description = "No credentials found"),
        (status = 500, description = "Internal server error"),
    ),
    tag = "authentication"
)]
#[instrument(name = "webauthn_auth_start", skip(state))]
pub async fn auth_start(
    State(state): State<gate_http::AppState<crate::MinimalState>>,
) -> Result<Json<AuthStartResponse>, HttpError> {
    let webauthn_service = state
        .data
        .daemon
        .get_webauthn_service()
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?
        .ok_or_else(|| HttpError::BadRequest("WebAuthn service not enabled".to_string()))?;

    let (challenge, session_id) = webauthn_service.start_authentication().await?;

    Ok(Json(AuthStartResponse {
        challenge,
        session_id,
    }))
}

/// Complete WebAuthn authentication
#[utoipa::path(
    post,
    path = "/auth/webauthn/authenticate/complete",
    request_body = AuthCompleteRequest,
    responses(
        (status = 200, description = "Authentication completed", body = AuthCompleteResponse),
        (status = 401, description = "Authentication failed"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error"),
    ),
    tag = "authentication"
)]
#[instrument(
    name = "webauthn_auth_complete",
    skip(state, request),
    fields(
        session_id = %request.session_id
    )
)]
pub async fn auth_complete(
    State(state): State<gate_http::AppState<crate::MinimalState>>,
    Json(request): Json<AuthCompleteRequest>,
) -> Result<Json<AuthCompleteResponse>, HttpError> {
    let webauthn_service = state
        .data
        .daemon
        .get_webauthn_service()
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?
        .ok_or_else(|| HttpError::BadRequest("WebAuthn service not enabled".to_string()))?;

    let auth_service = &state.data.auth_service;

    let (credential_id, counter) = webauthn_service
        .complete_authentication(request.session_id, request.credential)
        .await?;

    let response = auth_service
        .complete_authentication(credential_id.clone(), counter)
        .await?;

    info!(
        "User authenticated: {} ({})",
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
async fn get_current_user(
    State(state): State<gate_http::AppState<crate::MinimalState>>,
    identity: HttpIdentity,
) -> Result<Json<CurrentUser>, HttpError> {
    // Get user from database via daemon
    let state_backend = state
        .data
        .daemon
        .get_state_backend()
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;

    let user_data = state_backend
        .get_user(&identity.id)
        .await
        .map_err(|e| HttpError::InternalServerError(format!("Failed to get user: {e}")))?
        .ok_or_else(|| HttpError::AuthorizationFailed("User not found".to_string()))?;

    Ok(Json(user_data.into()))
}

/// Add custom auth routes
pub fn add_routes(
    router: OpenApiRouter<gate_http::AppState<crate::MinimalState>>,
) -> OpenApiRouter<gate_http::AppState<crate::MinimalState>> {
    router
        .routes(routes!(get_bootstrap_status))
        .routes(routes!(get_current_user))
        .routes(routes!(register_with_bootstrap))
        .routes(routes!(register_start))
        .routes(routes!(register_complete))
        .routes(routes!(auth_start))
        .routes(routes!(auth_complete))
}
