//! Configuration management routes

use crate::Settings;
use axum::{extract, response};
use gate_http::{
    error::HttpError,
    services::HttpIdentity,
    types::{ConfigResponse, ConfigUpdateRequest},
};

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
    extract::State(state): extract::State<gate_http::AppState<crate::MinimalState>>,
) -> Result<response::Json<ConfigResponse>, HttpError> {
    // Use daemon to get config with permission check
    let config = state
        .data
        .daemon
        .clone()
        .with_http_identity(&identity)
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?
        .get_config()
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;

    Ok(response::Json(ConfigResponse { config }))
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
    extract::State(state): extract::State<gate_http::AppState<crate::MinimalState>>,
    extract::Json(request): extract::Json<ConfigUpdateRequest>,
) -> Result<response::Json<ConfigResponse>, HttpError> {
    // Deserialize the new configuration
    let new_config: Settings = serde_json::from_value(request.config.clone())
        .map_err(|e| HttpError::BadRequest(format!("Invalid configuration: {e}")))?;

    // Use daemon to update config with permission check
    state
        .data
        .daemon
        .clone()
        .with_http_identity(&identity)
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?
        .update_config(new_config.clone())
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;

    // Get the redacted config to return
    let config = state
        .data
        .daemon
        .clone()
        .with_http_identity(&identity)
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?
        .get_config()
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;

    Ok(response::Json(ConfigResponse { config }))
}

/// Add config routes to an OpenAPI router
pub fn add_routes(
    mut router: utoipa_axum::router::OpenApiRouter<gate_http::AppState<crate::MinimalState>>,
) -> utoipa_axum::router::OpenApiRouter<gate_http::AppState<crate::MinimalState>> {
    router = router
        .routes(utoipa_axum::routes!(get_config))
        .routes(utoipa_axum::routes!(update_config));

    router
}
