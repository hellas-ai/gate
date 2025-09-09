//! Configuration management routes

use crate::Settings;
use axum::{
    Router, extract, response,
    routing::{get, put},
};
use gate_http::{
    error::HttpError,
    services::HttpIdentity,
    types::{ConfigResponse, ConfigUpdateRequest},
};

/// Get the full configuration
pub async fn get_config(
    identity: HttpIdentity,
    extract::State(state): extract::State<gate_http::AppState<crate::State>>,
) -> Result<response::Json<ConfigResponse>, HttpError> {
    // Use daemon to get config with permission check
    let settings = state
        .data
        .daemon
        .clone()
        .with_http_identity(&identity)
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?
        .get_config()
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;
    let config = serde_json::to_value(settings)
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;
    Ok(response::Json(ConfigResponse { config }))
}

/// Update the full configuration
pub async fn update_config(
    identity: HttpIdentity,
    extract::State(state): extract::State<gate_http::AppState<crate::State>>,
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
    let config = serde_json::to_value(new_config)
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;
    Ok(response::Json(ConfigResponse { config }))
}

/// Add config routes to a router
pub fn add_routes(
    router: Router<gate_http::AppState<crate::State>>,
) -> Router<gate_http::AppState<crate::State>> {
    router
        .route("/api/config", get(get_config))
        .route("/api/config", put(update_config))
}
