//! Admin user management routes - refactored version

use crate::helpers::{admin::AdminPermissionHelper, errors::ErrorMapExt};
use axum::{extract::{Path, State}, response::Json};
use gate_core::access::{Action, ObjectId, ObjectIdentity, ObjectKind, Permissions, TargetNamespace};
use gate_core::types::User;
use gate_http::{AppState, error::HttpError, services::HttpIdentity};
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};

// Re-use existing type definitions
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
    pub enabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub disabled_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<User> for UserInfo {
    fn from(user: User) -> Self {
        UserInfo {
            id: user.id,
            name: user.name,
            enabled: user.is_enabled(),
            created_at: user.created_at,
            updated_at: user.updated_at,
            disabled_at: user.disabled_at,
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

fn default_page() -> usize { 1 }
fn default_page_size() -> usize { 20 }

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateUserStatusRequest {
    pub enabled: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UpdateUserStatusResponse {
    pub user: UserInfo,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UserPermission {
    pub action: String,
    pub object: String,
    pub granted_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UserPermissionsResponse {
    pub permissions: Vec<UserPermission>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct GrantPermissionRequest {
    pub action: String,
    pub object: String,
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
    security(("bearer" = [])),
    tag = "admin"
)]
#[instrument(name = "list_users", skip(app_state), fields(page = %query.page, page_size = %query.page_size))]
pub async fn list_users(
    identity: HttpIdentity,
    State(app_state): State<AppState<crate::State>>,
    axum::extract::Query(query): axum::extract::Query<ListUsersQuery>,
) -> Result<Json<UserListResponse>, HttpError> {
    let helper = AdminPermissionHelper::new(&app_state.data.daemon, identity.clone()).await?;
    
    // Check permission
    helper.require_admin(Action::Read, &ObjectIdentity {
        namespace: TargetNamespace::System,
        kind: ObjectKind::Users,
        id: ObjectId::new("*"),
    }).await?;
    
    // Get and filter users
    let mut users = helper.state_backend.list_users().await.map_internal_error()?;
    
    if let Some(search) = &query.search {
        let search_lower = search.to_lowercase();
        users.retain(|u| {
            u.id.to_lowercase().contains(&search_lower)
                || u.name.as_ref().map_or(false, |n| n.to_lowercase().contains(&search_lower))
        });
    }
    
    let total = users.len();
    let offset = (query.page.saturating_sub(1)) * query.page_size;
    
    let users: Vec<UserInfo> = users.into_iter()
        .skip(offset)
        .take(query.page_size)
        .map(UserInfo::from)
        .collect();
    
    info!("Admin {} listed {} users", identity.id, users.len());
    
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
    params(("user_id" = String, Path, description = "User ID")),
    responses(
        (status = 200, description = "User details", body = UserInfo),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin access required"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(("bearer" = [])),
    tag = "admin"
)]
#[instrument(name = "get_user", skip(app_state), fields(target_user_id = %user_id))]
pub async fn get_user(
    identity: HttpIdentity,
    State(app_state): State<AppState<crate::State>>,
    Path(user_id): Path<String>,
) -> Result<Json<UserInfo>, HttpError> {
    let helper = AdminPermissionHelper::new(&app_state.data.daemon, identity.clone()).await?;
    
    helper.require_admin(Action::Read, &ObjectIdentity {
        namespace: TargetNamespace::System,
        kind: ObjectKind::User,
        id: ObjectId::new(user_id.clone()),
    }).await?;
    
    let user = helper.state_backend
        .get_user(&user_id)
        .await
        .map_internal_error()?
        .ok_or_else(|| HttpError::NotFound(format!("User {user_id} not found")))?;
    
    info!("Admin {} retrieved user {}", identity.id, user_id);
    Ok(Json(UserInfo::from(user)))
}

/// Delete a user (admin only)
#[utoipa::path(
    delete,
    path = "/api/admin/users/{user_id}",
    params(("user_id" = String, Path, description = "User ID")),
    responses(
        (status = 204, description = "User deleted"),
        (status = 400, description = "Bad request - cannot delete self"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin access required"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(("bearer" = [])),
    tag = "admin"
)]
#[instrument(name = "delete_user", skip(app_state), fields(target_user_id = %user_id))]
pub async fn delete_user(
    identity: HttpIdentity,
    State(app_state): State<AppState<crate::State>>,
    Path(user_id): Path<String>,
) -> Result<axum::http::StatusCode, HttpError> {
    // Prevent self-deletion
    if identity.id == user_id {
        warn!("User {} attempted to delete themselves", identity.id);
        return Err(HttpError::BadRequest("Cannot delete your own account".to_string()));
    }
    
    let helper = AdminPermissionHelper::new(&app_state.data.daemon, identity.clone()).await?;
    
    helper.require_admin(Action::Delete, &ObjectIdentity {
        namespace: TargetNamespace::System,
        kind: ObjectKind::User,
        id: ObjectId::new(user_id.clone()),
    }).await?;
    
    // Check user exists
    helper.state_backend
        .get_user(&user_id)
        .await
        .map_internal_error()?
        .ok_or_else(|| HttpError::NotFound(format!("User {user_id} not found")))?;
    
    // Delete user
    helper.state_backend
        .delete_user(&user_id)
        .await
        .map_internal_error_with_context("Failed to delete user")?;
    
    // Remove user permissions
    let user_permissions = helper.permission_manager
        .list_permissions(&user_id)
        .await
        .map_internal_error()?;
    
    for perm in user_permissions {
        if let Err(e) = helper.permission_manager.revoke(&user_id, &perm.action, &perm.object).await {
            warn!("Failed to revoke permission during user deletion: {}", e);
        }
    }
    
    info!("Admin {} deleted user {}", identity.id, user_id);
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Update user status (enable/disable)
#[utoipa::path(
    patch,
    path = "/api/admin/users/{user_id}/status",
    params(("user_id" = String, Path, description = "User ID")),
    request_body = UpdateUserStatusRequest,
    responses(
        (status = 200, description = "User status updated", body = UpdateUserStatusResponse),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin access required"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(("bearer" = [])),
    tag = "admin"
)]
#[instrument(name = "update_user_status", skip(app_state), fields(target_user_id = %user_id, enabled = %request.enabled))]
pub async fn update_user_status(
    identity: HttpIdentity,
    State(app_state): State<AppState<crate::State>>,
    Path(user_id): Path<String>,
    Json(request): Json<UpdateUserStatusRequest>,
) -> Result<Json<UpdateUserStatusResponse>, HttpError> {
    // Prevent self-disable
    if identity.id == user_id && !request.enabled {
        warn!("User {} attempted to disable themselves", identity.id);
        return Err(HttpError::BadRequest("Cannot disable your own account".to_string()));
    }
    
    let helper = AdminPermissionHelper::new(&app_state.data.daemon, identity.clone()).await?;
    
    helper.require_admin(Action::Update, &ObjectIdentity {
        namespace: TargetNamespace::System,
        kind: ObjectKind::User,
        id: ObjectId::new(user_id.clone()),
    }).await?;
    
    // Get and update user
    let mut user = helper.state_backend
        .get_user(&user_id)
        .await
        .map_internal_error()?
        .ok_or_else(|| HttpError::NotFound(format!("User {user_id} not found")))?;
    
    if request.enabled {
        user.disabled_at = None;
    } else {
        user.disabled_at = Some(chrono::Utc::now());
    }
    user.updated_at = chrono::Utc::now();
    
    helper.state_backend
        .update_user(&user)
        .await
        .map_internal_error_with_context("Failed to update user status")?;
    
    info!("Admin {} {} user {}", identity.id, if request.enabled { "enabled" } else { "disabled" }, user_id);
    
    Ok(Json(UpdateUserStatusResponse {
        user: UserInfo::from(user),
    }))
}

/// Get user permissions
#[utoipa::path(
    get,
    path = "/api/admin/users/{user_id}/permissions",
    params(("user_id" = String, Path, description = "User ID")),
    responses(
        (status = 200, description = "User permissions", body = UserPermissionsResponse),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin access required"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(("bearer" = [])),
    tag = "admin"
)]
#[instrument(name = "get_user_permissions", skip(app_state), fields(target_user_id = %user_id))]
pub async fn get_user_permissions(
    identity: HttpIdentity,
    State(app_state): State<AppState<crate::State>>,
    Path(user_id): Path<String>,
) -> Result<Json<UserPermissionsResponse>, HttpError> {
    let helper = AdminPermissionHelper::new(&app_state.data.daemon, identity.clone()).await?;
    
    helper.require_admin(Action::Read, &ObjectIdentity {
        namespace: TargetNamespace::System,
        kind: ObjectKind::Permissions,
        id: ObjectId::new(user_id.clone()),
    }).await?;
    
    // Check user exists
    helper.state_backend
        .get_user(&user_id)
        .await
        .map_internal_error()?
        .ok_or_else(|| HttpError::NotFound(format!("User {user_id} not found")))?;
    
    let permissions = helper.permission_manager
        .list_permissions(&user_id)
        .await
        .map_internal_error()?
        .into_iter()
        .map(|p| UserPermission {
            action: p.action.to_string(),
            object: format!("{}/{}/{}", p.object.namespace, p.object.kind, p.object.id),
            granted_at: p.granted_at,
        })
        .collect();
    
    Ok(Json(UserPermissionsResponse { permissions }))
}

/// Grant permission to user
#[utoipa::path(
    post,
    path = "/api/admin/users/{user_id}/permissions",
    params(("user_id" = String, Path, description = "User ID")),
    request_body = GrantPermissionRequest,
    responses(
        (status = 201, description = "Permission granted"),
        (status = 400, description = "Bad request - invalid permission"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin access required"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(("bearer" = [])),
    tag = "admin"
)]
#[instrument(name = "grant_user_permission", skip(app_state), fields(target_user_id = %user_id, action = %request.action, object = %request.object))]
pub async fn grant_user_permission(
    identity: HttpIdentity,
    State(app_state): State<AppState<crate::State>>,
    Path(user_id): Path<String>,
    Json(request): Json<GrantPermissionRequest>,
) -> Result<axum::http::StatusCode, HttpError> {
    let helper = AdminPermissionHelper::new(&app_state.data.daemon, identity.clone()).await?;
    
    helper.require_admin(Action::Create, &ObjectIdentity {
        namespace: TargetNamespace::System,
        kind: ObjectKind::Permissions,
        id: ObjectId::new(user_id.clone()),
    }).await?;
    
    // Check user exists
    helper.state_backend
        .get_user(&user_id)
        .await
        .map_internal_error()?
        .ok_or_else(|| HttpError::NotFound(format!("User {user_id} not found")))?;
    
    // Parse action and object
    let action = request.action.parse()
        .map_err(|_| HttpError::BadRequest(format!("Invalid action: {}", request.action)))?;
    
    let object = parse_object_identity(&request.object)
        .ok_or_else(|| HttpError::BadRequest(format!("Invalid object format: {}", request.object)))?;
    
    helper.permission_manager
        .grant(&user_id, action, &object)
        .await
        .map_internal_error_with_context("Failed to grant permission")?;
    
    info!("Admin {} granted {} permission on {} to user {}", identity.id, request.action, request.object, user_id);
    
    Ok(axum::http::StatusCode::CREATED)
}

/// Revoke permission from user
#[utoipa::path(
    delete,
    path = "/api/admin/users/{user_id}/permissions",
    params(
        ("user_id" = String, Path, description = "User ID"),
        ("action" = String, Query, description = "Action to revoke"),
        ("object" = String, Query, description = "Object to revoke permission on")
    ),
    responses(
        (status = 204, description = "Permission revoked"),
        (status = 400, description = "Bad request - invalid permission"),
        (status = 401, description = "Unauthorized"),
        (status = 403, description = "Forbidden - admin access required"),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(("bearer" = [])),
    tag = "admin"
)]
#[instrument(name = "revoke_user_permission", skip(app_state))]
pub async fn revoke_user_permission(
    identity: HttpIdentity,
    State(app_state): State<AppState<crate::State>>,
    Path(user_id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<axum::http::StatusCode, HttpError> {
    let action = params.get("action")
        .ok_or_else(|| HttpError::BadRequest("Missing action parameter".to_string()))?;
    let object = params.get("object")
        .ok_or_else(|| HttpError::BadRequest("Missing object parameter".to_string()))?;
    
    let helper = AdminPermissionHelper::new(&app_state.data.daemon, identity.clone()).await?;
    
    helper.require_admin(Action::Delete, &ObjectIdentity {
        namespace: TargetNamespace::System,
        kind: ObjectKind::Permissions,
        id: ObjectId::new(user_id.clone()),
    }).await?;
    
    // Check user exists
    helper.state_backend
        .get_user(&user_id)
        .await
        .map_internal_error()?
        .ok_or_else(|| HttpError::NotFound(format!("User {user_id} not found")))?;
    
    // Parse action and object
    let action_enum = action.parse()
        .map_err(|_| HttpError::BadRequest(format!("Invalid action: {}", action)))?;
    
    let object_identity = parse_object_identity(object)
        .ok_or_else(|| HttpError::BadRequest(format!("Invalid object format: {}", object)))?;
    
    helper.permission_manager
        .revoke(&user_id, &action_enum, &object_identity)
        .await
        .map_internal_error_with_context("Failed to revoke permission")?;
    
    info!("Admin {} revoked {} permission on {} from user {}", identity.id, action, object, user_id);
    
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// Helper function to parse object identity string
fn parse_object_identity(s: &str) -> Option<ObjectIdentity> {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() != 3 {
        return None;
    }
    
    Some(ObjectIdentity {
        namespace: parts[0].parse().ok()?,
        kind: parts[1].parse().ok()?,
        id: ObjectId::new(parts[2]),
    })
}

/// Register admin routes
pub fn register(router: OpenApiRouter) -> OpenApiRouter {
    router
        .routes(routes!(list_users))
        .routes(routes!(get_user))
        .routes(routes!(delete_user))
        .routes(routes!(update_user_status))
        .routes(routes!(get_user_permissions))
        .routes(routes!(grant_user_permission))
        .routes(routes!(revoke_user_permission))
}