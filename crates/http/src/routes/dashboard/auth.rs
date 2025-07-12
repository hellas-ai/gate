//! WebAuthn authentication endpoints

use crate::{
    error::HttpError,
    services::{AuthService, WebAuthnService},
    state::AppState,
    types::*,
};
use axum::{extract::State, response::Json};
use chrono::Utc;
use gate_core::User;
use tracing::{info, instrument};
use utoipa_axum::{router::OpenApiRouter, routes};

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
    skip(app_state),
    fields(
        user_name = %request.name
    )
)]
pub async fn register_start<T>(
    State(app_state): State<AppState<T>>,
    Json(request): Json<RegisterStartRequest>,
) -> Result<Json<RegisterStartResponse>, HttpError>
where
    T: Clone + Send + Sync + 'static + AsRef<std::sync::Arc<WebAuthnService>>,
{
    let webauthn_service: &std::sync::Arc<WebAuthnService> = app_state.data.as_ref().as_ref();

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
    skip(app_state, request),
    fields(
        session_id = %request.session_id,
        device_name = ?request.device_name,
        has_bootstrap_token = request.bootstrap_token.is_some()
    )
)]
pub async fn register_complete<T>(
    State(app_state): State<AppState<T>>,
    Json(request): Json<RegisterCompleteRequest>,
) -> Result<Json<RegisterCompleteResponse>, HttpError>
where
    T: Clone
        + Send
        + Sync
        + 'static
        + AsRef<std::sync::Arc<WebAuthnService>>
        + AsRef<std::sync::Arc<AuthService>>,
{
    let webauthn_service: &std::sync::Arc<WebAuthnService> = app_state.data.as_ref().as_ref();
    let auth_service: &std::sync::Arc<AuthService> = app_state.data.as_ref().as_ref();

    let device_name = request.device_name.clone();
    let (passkey, credential_id, user_name) = webauthn_service
        .complete_registration(request.session_id, request.credential)
        .await?;

    let role = if request.bootstrap_token.is_some() {
        // If bootstrap token is provided, create admin user
        "admin".to_string()
    } else {
        // Otherwise, create regular user
        "user".to_string()
    };

    // Create user with default role
    let user = User {
        id: credential_id.clone(),
        name: Some(user_name.clone()),
        role,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        metadata: std::collections::HashMap::new(),
    };

    let response = auth_service
        .complete_registration(user, credential_id.clone(), device_name, passkey)
        .await?;

    info!("User registered: {} ({})", user_name, credential_id);

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
#[instrument(name = "webauthn_auth_start", skip(app_state))]
pub async fn auth_start<T>(
    State(app_state): State<AppState<T>>,
) -> Result<Json<AuthStartResponse>, HttpError>
where
    T: Clone + Send + Sync + 'static + AsRef<std::sync::Arc<WebAuthnService>>,
{
    let webauthn_service: &std::sync::Arc<WebAuthnService> = app_state.data.as_ref().as_ref();

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
    skip(app_state, request),
    fields(
        session_id = %request.session_id
    )
)]
pub async fn auth_complete<T>(
    State(app_state): State<AppState<T>>,
    Json(request): Json<AuthCompleteRequest>,
) -> Result<Json<AuthCompleteResponse>, HttpError>
where
    T: Clone
        + Send
        + Sync
        + 'static
        + AsRef<std::sync::Arc<WebAuthnService>>
        + AsRef<std::sync::Arc<AuthService>>,
{
    let webauthn_service: &std::sync::Arc<WebAuthnService> = app_state.data.as_ref().as_ref();
    let auth_service: &std::sync::Arc<AuthService> = app_state.data.as_ref().as_ref();

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

/// Create the WebAuthn router
pub fn router<
    T: Send
        + Sync
        + Clone
        + 'static
        + AsRef<std::sync::Arc<WebAuthnService>>
        + AsRef<std::sync::Arc<AuthService>>,
>() -> OpenApiRouter<AppState<T>> {
    OpenApiRouter::new()
        .routes(routes!(register_start))
        .routes(routes!(register_complete))
        .routes(routes!(auth_start))
        .routes(routes!(auth_complete))
}
