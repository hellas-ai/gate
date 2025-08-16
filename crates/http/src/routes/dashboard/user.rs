//! User management endpoints

use crate::{error::HttpError, services::HttpIdentity, state::AppState};
use axum::{Extension, extract::State, response::Json};
use chrono::{DateTime, Utc};
use gate_core::access::IdentityContext;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

/// Dashboard user representation
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DashboardUser {
    /// User ID
    pub id: String,
    /// User email
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// User creation timestamp
    pub created_at: DateTime<Utc>,
}

/// User statistics response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UserStats {
    /// Number of API keys owned by the user
    pub api_keys_count: usize,
    /// User's account balance (credits)
    pub balance: f64,
}

/// Get current authenticated user
#[utoipa::path(
    get,
    path = "/user",
    responses(
        (status = 200, description = "Current user information", body = DashboardUser),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    tag = "users"
)]
#[instrument(
    name = "get_current_user",
    skip(_app_state),
    fields(
        user_id = %identity.id
    )
)]
pub async fn get_current_user<T>(
    Extension(identity): Extension<HttpIdentity>,
    State(_app_state): State<AppState<T>>,
) -> Result<Json<DashboardUser>, HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    // For now, we'll return a simple representation based on the authenticated user
    // In a real implementation, this would fetch additional details from the database
    let dashboard_user = DashboardUser {
        id: identity.id.clone(),
        email: identity.context.get("email").map(|s| s.to_string()),
        created_at: Utc::now(), // Placeholder - should come from database
    };

    Ok(Json(dashboard_user))
}

/// Get user statistics
#[utoipa::path(
    get,
    path = "/user/stats",
    responses(
        (status = 200, description = "User statistics", body = UserStats),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    tag = "users"
)]
#[instrument(
    name = "get_user_stats",
    skip(app_state),
    fields(
        user_id = %identity.id,
        api_keys_count = tracing::field::Empty
    )
)]
pub async fn get_user_stats<T>(
    Extension(identity): Extension<HttpIdentity>,
    State(app_state): State<AppState<T>>,
) -> Result<Json<UserStats>, HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    // Count the user's API keys
    let api_keys = app_state
        .state_backend
        .list_api_keys(&identity.id)
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;

    tracing::Span::current().record("api_keys_count", api_keys.len());

    let stats = UserStats {
        api_keys_count: api_keys.len(),
        balance: 0.0, // Placeholder - billing not implemented yet
    };

    Ok(Json(stats))
}

/// Create the user routes router
pub fn router<T: Send + Sync + Clone + 'static>() -> OpenApiRouter<AppState<T>> {
    OpenApiRouter::new()
        .routes(routes!(get_current_user))
        .routes(routes!(get_user_stats))
}
