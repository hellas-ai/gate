//! Configuration management routes

use crate::{ServerState, Settings, StateDir};
use axum::{extract, response, routing};
use gate_http::{
    AppState,
    error::HttpError,
    middleware::auth::AuthenticatedUser,
    types::{ConfigResponse, ConfigUpdateRequest},
};
use tracing::{info, warn};

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
    user: AuthenticatedUser,
    extract::State(state): extract::State<AppState<ServerState>>,
) -> Result<response::Json<ConfigResponse>, HttpError> {
    // Check if user has admin role
    // if !user.is_admin() {
    //     warn!(
    //         "Non-admin user {}  with roles {} attempted to get config",
    //         user.id,
    //         user.roles.join(", ")
    //     );
    //     return Err(HttpError::AuthorizationFailed(
    //         "Admin access required".to_string(),
    //     ));
    // }

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
    user: AuthenticatedUser,
    extract::State(_state): extract::State<AppState<ServerState>>,
    extract::Json(request): extract::Json<ConfigUpdateRequest>,
) -> Result<response::Json<ConfigResponse>, HttpError> {
    // if !user.is_admin() {
    //     warn!(
    //         "Non-admin user {} with roles {} attempted to update config",
    //         user.id,
    //     );
    //     return Err(HttpError::AuthorizationFailed(
    //         "Admin access required".to_string(),
    //     ));
    // }

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
