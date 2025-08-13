//! Admin user management routes

use crate::config::Settings;
use crate::permissions::LocalPermissionManager;
use axum::{extract::State, response::Json};
use gate_core::access::{
    Action, ObjectId, ObjectIdentity, ObjectKind, Permissions, TargetNamespace,
};
use gate_core::types::User;
use gate_http::{error::HttpError, services::HttpIdentity, state::AppState};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, instrument, warn};
use utoipa_axum::{router::OpenApiRouter, routes};

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UserListResponse {
    pub users: Vec<UserInfo>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UserInfo {
    pub id: String,
    pub name: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<User> for UserInfo {
    fn from(user: User) -> Self {
        UserInfo {
            id: user.id,
            name: user.name,
            created_at: user.created_at,
            updated_at: user.updated_at,
        }
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct ListUsersQuery {
    #[serde(default = "default_page")]
    pub page: usize,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
    pub search: Option<String>,
}

fn default_page() -> usize {
    1
}

fn default_page_size() -> usize {
    20
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateUserRoleRequest {
    pub role: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UpdateUserRoleResponse {
    pub user: UserInfo,
}

/// List all users (admin only)
#[utoipa::path(
    get,
    path = "/api/admin/users",
    params(ListUsersQuery),
    responses(
        (status = 200, description = "List of users", body = UserListResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin access required"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("bearer" = [])
    ),
    tag = "admin"
)]
#[instrument(
    name = "list_users",
    skip(app_state),
    fields(
        page = %query.page,
        page_size = %query.page_size,
        search = ?query.search
    )
)]
pub async fn list_users<T>(
    identity: HttpIdentity,
    State(app_state): State<AppState<T>>,
    axum::extract::Query(query): axum::extract::Query<ListUsersQuery>,
) -> Result<Json<UserListResponse>, HttpError>
where
    T: Clone + Send + Sync + 'static + AsRef<Arc<Settings>> + AsRef<Arc<LocalPermissionManager>>,
{
    let permission_manager: &Arc<LocalPermissionManager> = app_state.data.as_ref().as_ref();

    // Check permission to read users
    let users_object = ObjectIdentity {
        namespace: TargetNamespace::System,
        kind: ObjectKind::Users,
        id: ObjectId::new("*"),
    };

    let local_ctx = crate::permissions::LocalContext::from_http_identity(
        &identity,
        app_state.state_backend.as_ref(),
    )
    .await;

    let local_identity = gate_core::access::SubjectIdentity::new(
        identity.id.clone(),
        identity.source.clone(),
        local_ctx,
    );

    if let Err(_) = permission_manager
        .check(&local_identity, Action::Read, &users_object)
        .await
    {
        warn!(
            "User {} attempted to list users without permission",
            identity.id
        );
        return Err(HttpError::AuthorizationFailed(
            "Permission denied: cannot list users".to_string(),
        ));
    }

    // Calculate offset
    let offset = (query.page.saturating_sub(1)) * query.page_size;

    // Get users from state backend
    let mut all_users = app_state
        .state_backend
        .list_users()
        .await
        .map_err(|e| HttpError::InternalServerError(format!("Failed to list users: {e}")))?;

    if let Some(search_term) = &query.search {
        let search_lower = search_term.to_lowercase();
        all_users.retain(|u| {
            u.id.to_lowercase().contains(&search_lower)
                || u.name
                    .as_ref()
                    .map(|n| n.to_lowercase().contains(&search_lower))
                    .unwrap_or(false)
        });
    }

    let total = all_users.len();

    // Apply pagination
    let users: Vec<UserInfo> = all_users
        .into_iter()
        .skip(offset)
        .take(query.page_size)
        .map(UserInfo::from)
        .collect();

    info!(
        "Admin user {} listed {} users (page {}/{})",
        identity.id,
        users.len(),
        query.page,
        total.div_ceil(query.page_size)
    );

    Ok(Json(UserListResponse {
        users,
        total,
        page: query.page,
        page_size: query.page_size,
    }))
}

/// Get a specific user (admin only)
#[utoipa::path(
    get,
    path = "/api/admin/users/{user_id}",
    params(
        ("user_id" = String, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User details", body = UserInfo),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin access required"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("bearer" = [])
    ),
    tag = "admin"
)]
#[instrument(name = "get_user", skip(app_state), fields(target_user_id = %user_id))]
pub async fn get_user<T>(
    identity: HttpIdentity,
    State(app_state): State<AppState<T>>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> Result<Json<UserInfo>, HttpError>
where
    T: Clone + Send + Sync + 'static + AsRef<Arc<Settings>> + AsRef<Arc<LocalPermissionManager>>,
{
    let permission_manager: &Arc<LocalPermissionManager> = app_state.data.as_ref().as_ref();

    // Check permission to read specific user
    let user_object = ObjectIdentity {
        namespace: TargetNamespace::System,
        kind: ObjectKind::User,
        id: ObjectId::new(user_id.clone()),
    };

    let local_ctx = crate::permissions::LocalContext::from_http_identity(
        &identity,
        app_state.state_backend.as_ref(),
    )
    .await;
    let local_identity = gate_core::access::SubjectIdentity::new(
        identity.id.clone(),
        identity.source.clone(),
        local_ctx,
    );

    if let Err(_) = permission_manager
        .check(&local_identity, Action::Read, &user_object)
        .await
    {
        warn!(
            "User {} attempted to get user {} without permission",
            identity.id, user_id
        );
        return Err(HttpError::AuthorizationFailed(
            "Permission denied: cannot read user".to_string(),
        ));
    }

    // Get user from state backend
    let target_user = app_state
        .state_backend
        .get_user(&user_id)
        .await
        .map_err(|e| HttpError::InternalServerError(format!("Failed to get user: {e}")))?
        .ok_or_else(|| HttpError::NotFound(format!("User {user_id} not found")))?;

    info!(
        "Admin user {} retrieved details for user {}",
        identity.id, user_id
    );

    Ok(Json(UserInfo::from(target_user)))
}

/// Update a user's role (admin only)
// #[utoipa::path(
//     put,
//     path = "/api/admin/users/{user_id}/role",
//     params(
//         ("user_id" = String, Path, description = "User ID")
//     ),
//     request_body = UpdateUserRoleRequest,
//     responses(
//         (status = 200, description = "User role updated", body = UpdateUserRoleResponse),
//         (status = 400, description = "Bad request"),
//         (status = 401, description = "Unauthorized"),
//         (status = 403, description = "Forbidden - admin access required"),
//         (status = 404, description = "User not found"),
//         (status = 500, description = "Internal server error"),
//     ),
//     security(
//         ("bearer" = [])
//     ),
//     tag = "admin"
// )]
// #[instrument(
//     name = "update_user_role",
//     skip(app_state),
//     fields(
//         target_user_id = %user_id,
//         new_role = %request.role
//     )
// )]
// pub async fn update_user_role<T>(
//     identity: HttpIdentity,
//     State(app_state): State<AppState<T>>,
//     axum::extract::Path(user_id): axum::extract::Path<String>,
//     Json(request): Json<UpdateUserRoleRequest>,
// ) -> Result<Json<UpdateUserRoleResponse>, HttpError>
// where
//     T: Clone + Send + Sync + 'static + AsRef<Arc<Settings>>,
// {
//     let settings: &Arc<Settings> = app_state.data.as_ref().as_ref();

//     // Check admin role
//     let is_admin = settings
//         .auth
//         .registration
//         .admin_roles
//         .iter()
//         .any(|admin_role| user.roles.contains(admin_role));

//     if !is_admin {
//         warn!(
//             "Non-admin user {} attempted to update role for user {}",
//             identity.id, user_id
//         );
//         return Err(HttpError::AuthorizationFailed(
//             "Admin access required".to_string(),
//         ));
//     }

//     // Prevent self-demotion
//     if user.id == user_id
//         && !settings
//             .auth
//             .registration
//             .admin_roles
//             .contains(&request.role)
//     {
//         warn!(
//             "Admin user {} attempted to remove their own admin role",
//             user.id
//         );
//         return Err(HttpError::BadRequest(
//             "Cannot remove your own admin role".to_string(),
//         ));
//     }

//     // Get the user
//     let mut target_user = app_state
//         .state_backend
//         .get_user(&user_id)
//         .await
//         .map_err(|e| HttpError::InternalServerError(format!("Failed to get user: {e}")))?
//         .ok_or_else(|| HttpError::NotFound(format!("User {user_id} not found")))?;

//     // Update the role
//     let old_role = target_user.role.clone();
//     target_user.role = request.role;
//     target_user.updated_at = chrono::Utc::now();

//     // Save the updated user
//     app_state
//         .state_backend
//         .update_user(&target_user)
//         .await
//         .map_err(|e| HttpError::InternalServerError(format!("Failed to update user: {e}")))?;

//     info!(
//         "Admin user {} updated role for user {} from '{}' to '{}'",
//         identity.id, user_id, old_role, target_user.role
//     );

//     Ok(Json(UpdateUserRoleResponse {
//         user: UserInfo::from(target_user),
//     }))
// }

/// Delete a user (admin only)
#[utoipa::path(
    delete,
    path = "/api/admin/users/{user_id}",
    params(
        ("user_id" = String, Path, description = "User ID")
    ),
    responses(
        (status = 204, description = "User deleted"),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin access required"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("bearer" = [])
    ),
    tag = "admin"
)]
#[instrument(name = "delete_user", skip(app_state), fields(target_user_id = %user_id))]
pub async fn delete_user<T>(
    identity: HttpIdentity,
    State(app_state): State<AppState<T>>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> Result<axum::response::Response, HttpError>
where
    T: Clone + Send + Sync + 'static + AsRef<Arc<Settings>> + AsRef<Arc<LocalPermissionManager>>,
{
    let permission_manager: &Arc<LocalPermissionManager> = app_state.data.as_ref().as_ref();

    // Check permission to delete user
    let user_object = ObjectIdentity {
        namespace: TargetNamespace::System,
        kind: ObjectKind::User,
        id: ObjectId::new(user_id.clone()),
    };

    let local_ctx = crate::permissions::LocalContext::from_http_identity(
        &identity,
        app_state.state_backend.as_ref(),
    )
    .await;
    let local_identity = gate_core::access::SubjectIdentity::new(
        identity.id.clone(),
        identity.source.clone(),
        local_ctx,
    );

    if let Err(_) = permission_manager
        .check(&local_identity, Action::Delete, &user_object)
        .await
    {
        warn!(
            "User {} attempted to delete user {} without permission",
            identity.id, user_id
        );
        return Err(HttpError::AuthorizationFailed(
            "Permission denied: cannot delete user".to_string(),
        ));
    }

    // Prevent self-deletion
    if identity.id == user_id {
        warn!("Admin user {} attempted to delete themselves", identity.id);
        return Err(HttpError::BadRequest(
            "Cannot delete your own account".to_string(),
        ));
    }

    // Check if user exists
    let target_user = app_state
        .state_backend
        .get_user(&user_id)
        .await
        .map_err(|e| HttpError::InternalServerError(format!("Failed to get user: {e}")))?
        .ok_or_else(|| HttpError::NotFound(format!("User {user_id} not found")))?;

    // Delete the user
    app_state
        .state_backend
        .delete_user(&user_id)
        .await
        .map_err(|e| HttpError::InternalServerError(format!("Failed to delete user: {e}")))?;

    info!(
        "Admin user {} deleted user {} ({})",
        identity.id,
        user_id,
        target_user.name.unwrap_or_else(|| "unnamed".to_string())
    );

    Ok(axum::response::Response::builder()
        .status(204)
        .body(axum::body::Body::empty())
        .unwrap())
}

/// Add admin routes
pub fn add_routes<T>(router: OpenApiRouter<AppState<T>>) -> OpenApiRouter<AppState<T>>
where
    T: Clone + Send + Sync + 'static + AsRef<Arc<Settings>> + AsRef<Arc<LocalPermissionManager>>,
{
    router
        .routes(routes!(list_users))
        .routes(routes!(get_user))
        // .routes(routes!(update_user_role))
        .routes(routes!(delete_user))
}
