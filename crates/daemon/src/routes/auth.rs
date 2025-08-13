//! Custom authentication routes with registration control

use crate::bootstrap::BootstrapTokenManager;
use crate::config::Settings;
use axum::{extract::State, response::Json};
use chrono::{DateTime, Utc};
use gate_core::{BootstrapTokenValidator, User};
use gate_http::{
    error::HttpError,
    services::{AuthService, HttpIdentity, WebAuthnService},
    state::AppState,
};
use std::sync::Arc;
use tracing::instrument;
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
        + AsRef<Arc<BootstrapTokenManager>>,
{
    router
        .routes(routes!(get_bootstrap_status))
        .routes(routes!(get_current_user))
}
