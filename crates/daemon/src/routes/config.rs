//! Configuration management routes

use crate::permissions::LocalContext;
use crate::{ServerState, Settings, StateDir};
use axum::{extract, response, routing};
use gate_core::access::{
    Action, ObjectId, ObjectIdentity, ObjectKind, Permissions, SubjectIdentity, TargetNamespace,
};
use gate_http::{
    AppState,
    error::HttpError,
    services::HttpIdentity,
    types::{ConfigResponse, ConfigUpdateRequest},
};
use tracing::info;

/// Get the full configuration
#[utoipa::path(
    get,
    path = "/api/config",
    responses(
        (status = 200, description = "Current configuration", body = ConfigResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer" = [])
    ),
    tag = "config"
)]
pub async fn get_config(
    identity: HttpIdentity,
    extract::State(state): extract::State<AppState<ServerState>>,
) -> Result<response::Json<ConfigResponse>, HttpError> {
    // Check permission to read configuration
    let permission_manager = &state.data.permission_manager;
    let config_object = ObjectIdentity {
        namespace: TargetNamespace::System,
        kind: ObjectKind::Config,
        id: ObjectId::new("*"),
    };

    let local_ctx = LocalContext::from_http_identity(&identity, state.state_backend.as_ref()).await;
    let local_identity =
        SubjectIdentity::new(identity.id.clone(), identity.source.clone(), local_ctx);

    if permission_manager
        .check(&local_identity, Action::Read, &config_object)
        .await
        .is_err()
    {
        return Err(HttpError::AuthorizationFailed(
            "Permission denied: cannot read configuration".to_string(),
        ));
    }

    // Get the current configuration from state
    let current_config = &*state.data.settings;

    // Convert to JSON, redacting sensitive fields
    let mut config_json = serde_json::to_value(current_config)
        .map_err(|e| HttpError::InternalServerError(format!("Failed to serialize config: {e}")))?;

    // Redact sensitive fields
    if let Some(upstreams) = config_json
        .get_mut("upstreams")
        .and_then(|v| v.as_array_mut())
    {
        for upstream in upstreams {
            if let Some(api_key) = upstream.get_mut("api_key")
                && api_key.as_str().is_some()
            {
                *api_key = serde_json::json!("<redacted>");
            }
        }
    }

    // Redact JWT secret
    if let Some(auth) = config_json.get_mut("auth")
        && let Some(jwt) = auth.get_mut("jwt")
        && let Some(secret) = jwt.get_mut("secret")
        && secret.as_str().is_some()
    {
        *secret = serde_json::json!("<redacted>");
    }

    Ok(response::Json(ConfigResponse {
        config: config_json,
    }))
}

/// Update the full configuration
#[utoipa::path(
    put,
    path = "/api/config",
    request_body = ConfigUpdateRequest,
    responses(
        (status = 200, description = "Configuration updated", body = ConfigResponse),
        (status = 400, description = "Invalid configuration"),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    security(
        ("bearer" = [])
    ),
    tag = "config"
)]
pub async fn update_config(
    identity: HttpIdentity,
    extract::State(_state): extract::State<AppState<ServerState>>,
    extract::Json(request): extract::Json<ConfigUpdateRequest>,
) -> Result<response::Json<ConfigResponse>, HttpError> {
    // Check permission to write configuration
    let permission_manager = &_state.data.permission_manager;
    let config_object = ObjectIdentity {
        namespace: TargetNamespace::System,
        kind: ObjectKind::Config,
        id: ObjectId::new("*"),
    };

    let local_ctx = crate::permissions::LocalContext::from_http_identity(
        &identity,
        _state.state_backend.as_ref(),
    )
    .await;
    let local_identity =
        SubjectIdentity::new(identity.id.clone(), identity.source.clone(), local_ctx);

    if permission_manager
        .check(&local_identity, Action::Write, &config_object)
        .await
        .is_err()
    {
        return Err(HttpError::AuthorizationFailed(
            "Permission denied: cannot update configuration".to_string(),
        ));
    }

    // Deserialize the new configuration
    let new_config: Settings = serde_json::from_value(request.config.clone())
        .map_err(|e| HttpError::BadRequest(format!("Invalid configuration: {e}")))?;

    // Write to runtime config file
    let state_dir = StateDir::new();
    let config_path = state_dir.config_path();

    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            HttpError::InternalServerError(format!("Failed to create config directory: {e}"))
        })?;
    }

    // Write the configuration as JSON
    let config_string = serde_json::to_string_pretty(&new_config)
        .map_err(|e| HttpError::InternalServerError(format!("Failed to serialize config: {e}")))?;

    tokio::fs::write(&config_path, config_string)
        .await
        .map_err(|e| HttpError::InternalServerError(format!("Failed to write config file: {e}")))?;

    info!(
        "Wrote updated config to {}. Restart required to apply changes.",
        config_path.display()
    );

    // Return the updated configuration (with sensitive fields redacted)
    let mut config_json = serde_json::to_value(&new_config)
        .map_err(|e| HttpError::InternalServerError(format!("Failed to serialize config: {e}")))?;

    // Redact sensitive fields
    if let Some(upstreams) = config_json
        .get_mut("upstreams")
        .and_then(|v| v.as_array_mut())
    {
        for upstream in upstreams {
            if let Some(api_key) = upstream.get_mut("api_key")
                && api_key.as_str().is_some()
            {
                *api_key = serde_json::json!("<redacted>");
            }
        }
    }

    // Redact JWT secret
    if let Some(auth) = config_json.get_mut("auth")
        && let Some(jwt) = auth.get_mut("jwt")
        && let Some(secret) = jwt.get_mut("secret")
        && secret.as_str().is_some()
    {
        *secret = serde_json::json!("<redacted>");
    }

    Ok(response::Json(ConfigResponse {
        config: config_json,
    }))
}

/// Create the configuration routes
pub fn router() -> axum::Router<AppState<ServerState>> {
    axum::Router::new().route("/api/config", routing::get(get_config).put(update_config))
    // Note: auth middleware is applied when converting to the final router
}

/// Add config routes to an OpenAPI router
pub fn add_routes(
    mut router: utoipa_axum::router::OpenApiRouter<AppState<ServerState>>,
) -> utoipa_axum::router::OpenApiRouter<AppState<ServerState>> {
    router = router
        .routes(utoipa_axum::routes!(get_config))
        .routes(utoipa_axum::routes!(update_config));

    router
}
