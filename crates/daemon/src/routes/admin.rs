//! Admin user management routes - refactored version

use crate::helpers::{errors::ErrorMapExt, services::ServiceAccessor};
use crate::permissions::LocalContext;
use axum::{
    Router,
    extract::{Path, State},
    response::Json,
    routing::{get, patch},
};
use gate_core::access::{
    Action, ObjectId, ObjectIdentity, ObjectKind, Permissions, SubjectIdentity, TargetNamespace,
};
use gate_core::state::StateBackend;
use gate_core::types::User;
use gate_http::{AppState, error::HttpError, services::HttpIdentity};
use serde::{Deserialize, Serialize};

// Re-use existing type definitions
#[derive(Debug, Serialize, Deserialize)]
pub struct UserListResponse {
    pub users: Vec<UserInfo>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}

#[derive(Debug, Serialize, Deserialize)]
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
        let enabled = user.is_enabled();
        UserInfo {
            id: user.id,
            name: user.name,
            enabled,
            created_at: user.created_at,
            updated_at: user.updated_at,
            disabled_at: user.disabled_at,
        }
    }
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
pub struct UpdateUserStatusRequest {
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct UpdateUserStatusResponse {
    pub user: UserInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserPermission {
    pub action: String,
    pub object: String,
    pub granted_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct UserPermissionsResponse {
    pub permissions: Vec<UserPermission>,
}

#[derive(Debug, Deserialize)]
pub struct GrantPermissionRequest {
    pub action: String,
    pub object: String,
}

async fn to_local_identity(
    identity: &HttpIdentity,
    state_backend: &dyn StateBackend,
) -> SubjectIdentity<LocalContext> {
    let local_ctx = LocalContext::from_http_identity(identity, state_backend).await;
    SubjectIdentity::new(identity.id.clone(), identity.source.clone(), local_ctx)
}

async fn require_admin_permission(
    permissions: &crate::permissions::LocalPermissionManager,
    identity: &SubjectIdentity<LocalContext>,
    action: Action,
    object: &ObjectIdentity,
) -> Result<(), HttpError> {
    permissions
        .require(identity, action, object)
        .await
        .map_err(|e| {
            tracing::debug!(
                "Permission denied for user {} on object {:?}: {}",
                identity.id,
                object,
                e
            );
            HttpError::AuthorizationFailed(format!("Insufficient permissions: {e}"))
        })
}

/// List all users (admin only)
#[instrument(name = "list_users", skip(app_state), fields(page = %query.page, page_size = %query.page_size))]
pub async fn list_users(
    identity: HttpIdentity,
    State(app_state): State<AppState<crate::State>>,
    axum::extract::Query(query): axum::extract::Query<ListUsersQuery>,
) -> Result<Json<UserListResponse>, HttpError> {
    let services = ServiceAccessor::new(&app_state.data.daemon);
    let (permissions, state_backend) = services.core_services().await?;
    let local_identity = to_local_identity(&identity, state_backend.as_ref()).await;

    // Check permission
    require_admin_permission(
        &permissions,
        &local_identity,
        Action::Read,
        &ObjectIdentity {
            namespace: TargetNamespace::System,
            kind: ObjectKind::Users,
            id: ObjectId::new("*"),
        },
    )
    .await?;

    // Get and filter users
    let mut users = state_backend.list_users().await.map_internal_error()?;

    if let Some(search) = &query.search {
        let search_lower = search.to_lowercase();
        users.retain(|u| {
            u.id.to_lowercase().contains(&search_lower)
                || u.name
                    .as_ref()
                    .is_some_and(|n| n.to_lowercase().contains(&search_lower))
        });
    }

    let total = users.len();
    let offset = (query.page.saturating_sub(1)) * query.page_size;

    let users: Vec<UserInfo> = users
        .into_iter()
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
#[instrument(name = "get_user", skip(app_state), fields(target_user_id = %user_id))]
pub async fn get_user(
    identity: HttpIdentity,
    State(app_state): State<AppState<crate::State>>,
    Path(user_id): Path<String>,
) -> Result<Json<UserInfo>, HttpError> {
    let services = ServiceAccessor::new(&app_state.data.daemon);
    let (permissions, state_backend) = services.core_services().await?;
    let local_identity = to_local_identity(&identity, state_backend.as_ref()).await;

    require_admin_permission(
        &permissions,
        &local_identity,
        Action::Read,
        &ObjectIdentity {
            namespace: TargetNamespace::System,
            kind: ObjectKind::User,
            id: ObjectId::new(user_id.clone()),
        },
    )
    .await?;

    let user = state_backend
        .get_user(&user_id)
        .await
        .map_internal_error()?
        .ok_or_else(|| HttpError::NotFound(format!("User {user_id} not found")))?;

    info!("Admin {} retrieved user {}", identity.id, user_id);
    Ok(Json(UserInfo::from(user)))
}

/// Delete a user (admin only)
#[instrument(name = "delete_user", skip(app_state), fields(target_user_id = %user_id))]
pub async fn delete_user(
    identity: HttpIdentity,
    State(app_state): State<AppState<crate::State>>,
    Path(user_id): Path<String>,
) -> Result<axum::http::StatusCode, HttpError> {
    // Prevent self-deletion
    if identity.id == user_id {
        warn!("User {} attempted to delete themselves", identity.id);
        return Err(HttpError::BadRequest(
            "Cannot delete your own account".to_string(),
        ));
    }

    let services = ServiceAccessor::new(&app_state.data.daemon);
    let (permissions, state_backend) = services.core_services().await?;
    let local_identity = to_local_identity(&identity, state_backend.as_ref()).await;

    require_admin_permission(
        &permissions,
        &local_identity,
        Action::Delete,
        &ObjectIdentity {
            namespace: TargetNamespace::System,
            kind: ObjectKind::User,
            id: ObjectId::new(user_id.clone()),
        },
    )
    .await?;

    // Check user exists
    state_backend
        .get_user(&user_id)
        .await
        .map_internal_error()?
        .ok_or_else(|| HttpError::NotFound(format!("User {user_id} not found")))?;

    // Delete user
    state_backend
        .delete_user(&user_id)
        .await
        .map_internal_error_with_context("Failed to delete user")?;

    info!("Admin {} deleted user {}", identity.id, user_id);
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// Update user status (enable/disable)
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
        return Err(HttpError::BadRequest(
            "Cannot disable your own account".to_string(),
        ));
    }

    let services = ServiceAccessor::new(&app_state.data.daemon);
    let (permissions, state_backend) = services.core_services().await?;
    let local_identity = to_local_identity(&identity, state_backend.as_ref()).await;

    require_admin_permission(
        &permissions,
        &local_identity,
        Action::Write,
        &ObjectIdentity {
            namespace: TargetNamespace::System,
            kind: ObjectKind::User,
            id: ObjectId::new(user_id.clone()),
        },
    )
    .await?;

    // Get and update user
    let mut user = state_backend
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

    state_backend
        .update_user(&user)
        .await
        .map_internal_error_with_context("Failed to update user status")?;

    info!(
        "Admin {} {} user {}",
        identity.id,
        if request.enabled {
            "enabled"
        } else {
            "disabled"
        },
        user_id
    );

    Ok(Json(UpdateUserStatusResponse {
        user: UserInfo::from(user),
    }))
}

/// Get user permissions
#[instrument(name = "get_user_permissions", skip(app_state), fields(target_user_id = %user_id))]
pub async fn get_user_permissions(
    identity: HttpIdentity,
    State(app_state): State<AppState<crate::State>>,
    Path(user_id): Path<String>,
) -> Result<Json<UserPermissionsResponse>, HttpError> {
    let services = ServiceAccessor::new(&app_state.data.daemon);
    let (permissions, state_backend) = services.core_services().await?;
    let local_identity = to_local_identity(&identity, state_backend.as_ref()).await;

    require_admin_permission(
        &permissions,
        &local_identity,
        Action::ViewPermissions,
        &ObjectIdentity {
            namespace: TargetNamespace::System,
            kind: ObjectKind::User,
            id: ObjectId::new(user_id.clone()),
        },
    )
    .await?;

    // Check user exists
    state_backend
        .get_user(&user_id)
        .await
        .map_internal_error()?
        .ok_or_else(|| HttpError::NotFound(format!("User {user_id} not found")))?;

    let permissions = state_backend
        .list_user_permissions(&user_id)
        .await
        .map_internal_error()?
        .into_iter()
        .map(|(action, object, granted_at)| UserPermission {
            action,
            object,
            granted_at,
        })
        .collect();

    Ok(Json(UserPermissionsResponse { permissions }))
}

/// Grant permission to user
#[instrument(name = "grant_user_permission", skip(app_state), fields(target_user_id = %user_id, action = %request.action, object = %request.object))]
pub async fn grant_user_permission(
    identity: HttpIdentity,
    State(app_state): State<AppState<crate::State>>,
    Path(user_id): Path<String>,
    Json(request): Json<GrantPermissionRequest>,
) -> Result<axum::http::StatusCode, HttpError> {
    let services = ServiceAccessor::new(&app_state.data.daemon);
    let (permissions, state_backend) = services.core_services().await?;
    let local_identity = to_local_identity(&identity, state_backend.as_ref()).await;

    require_admin_permission(
        &permissions,
        &local_identity,
        Action::GrantPermission,
        &ObjectIdentity {
            namespace: TargetNamespace::System,
            kind: ObjectKind::User,
            id: ObjectId::new(user_id.clone()),
        },
    )
    .await?;

    // Parse action and object
    let action = parse_action(&request.action)
        .ok_or_else(|| HttpError::BadRequest(format!("Invalid action: {}", request.action)))?;

    let object = parse_object_identity(&request.object).ok_or_else(|| {
        HttpError::BadRequest(format!("Invalid object format: {}", request.object))
    })?;

    // Create identity for the user receiving the permission
    let grantee_user = state_backend
        .get_user(&user_id)
        .await
        .map_internal_error()?
        .ok_or_else(|| HttpError::NotFound(format!("User {user_id} not found")))?;

    let grantee_identity = gate_core::access::SubjectIdentity::new(
        grantee_user.id.clone(),
        "user",
        crate::permissions::LocalContext {
            is_owner: false,
            node_id: "local".to_string(),
        },
    );

    permissions
        .grant(&local_identity, &grantee_identity, action, &object)
        .await
        .map_internal_error_with_context("Failed to grant permission")?;

    info!(
        "Admin {} granted {} permission on {} to user {}",
        identity.id, request.action, request.object, user_id
    );

    Ok(axum::http::StatusCode::CREATED)
}

/// Revoke permission from user
#[instrument(name = "revoke_user_permission", skip(app_state))]
pub async fn revoke_user_permission(
    identity: HttpIdentity,
    State(app_state): State<AppState<crate::State>>,
    Path(user_id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<axum::http::StatusCode, HttpError> {
    let action = params
        .get("action")
        .ok_or_else(|| HttpError::BadRequest("Missing action parameter".to_string()))?;
    let object = params
        .get("object")
        .ok_or_else(|| HttpError::BadRequest("Missing object parameter".to_string()))?;

    let services = ServiceAccessor::new(&app_state.data.daemon);
    let (permissions, state_backend) = services.core_services().await?;
    let local_identity = to_local_identity(&identity, state_backend.as_ref()).await;

    require_admin_permission(
        &permissions,
        &local_identity,
        Action::RevokePermission,
        &ObjectIdentity {
            namespace: TargetNamespace::System,
            kind: ObjectKind::User,
            id: ObjectId::new(user_id.clone()),
        },
    )
    .await?;

    // Check user exists and get their identity
    let target_user = state_backend
        .get_user(&user_id)
        .await
        .map_internal_error()?
        .ok_or_else(|| HttpError::NotFound(format!("User {user_id} not found")))?;

    // Parse action and object
    let action_enum = parse_action(action)
        .ok_or_else(|| HttpError::BadRequest(format!("Invalid action: {action}")))?;

    let object_identity = parse_object_identity(object)
        .ok_or_else(|| HttpError::BadRequest(format!("Invalid object format: {object}")))?;

    // Create identity for the user losing the permission
    let subject_identity = gate_core::access::SubjectIdentity::new(
        target_user.id.clone(),
        "user",
        crate::permissions::LocalContext {
            is_owner: false,
            node_id: "local".to_string(),
        },
    );

    permissions
        .revoke(
            &local_identity,
            &subject_identity,
            action_enum,
            &object_identity,
        )
        .await
        .map_internal_error_with_context("Failed to revoke permission")?;

    info!(
        "Admin {} revoked {} permission on {} from user {}",
        identity.id, action, object, user_id
    );

    Ok(axum::http::StatusCode::NO_CONTENT)
}

// Helper function to parse action string
fn parse_action(s: &str) -> Option<Action> {
    match s {
        "read" | "Read" => Some(Action::Read),
        "write" | "Write" => Some(Action::Write),
        "delete" | "Delete" => Some(Action::Delete),
        "execute" | "Execute" => Some(Action::Execute),
        "manage" | "Manage" => Some(Action::Manage),
        "grant_permission" | "GrantPermission" => Some(Action::GrantPermission),
        "revoke_permission" | "RevokePermission" => Some(Action::RevokePermission),
        "view_permissions" | "ViewPermissions" => Some(Action::ViewPermissions),
        "view_quota" | "ViewQuota" => Some(Action::ViewQuota),
        "update_quota" | "UpdateQuota" => Some(Action::UpdateQuota),
        _ => None,
    }
}

// Helper function to parse object identity string
fn parse_object_identity(s: &str) -> Option<ObjectIdentity> {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() != 3 {
        return None;
    }

    let namespace = match parts[0] {
        "system" => TargetNamespace::System,
        "local" => TargetNamespace::Local,
        _ => return None,
    };

    let kind = match parts[1] {
        "model" => ObjectKind::Model,
        "provider" => ObjectKind::Provider,
        "user" => ObjectKind::User,
        "users" => ObjectKind::Users,
        "config" => ObjectKind::Config,
        "billing" => ObjectKind::Billing,
        "system" => ObjectKind::System,
        "quota" => ObjectKind::Quota,
        _ => return None,
    };

    Some(ObjectIdentity {
        namespace,
        kind,
        id: ObjectId::new(parts[2]),
    })
}

/// Add admin routes
pub fn add_routes(
    router: Router<gate_http::AppState<crate::State>>,
) -> Router<gate_http::AppState<crate::State>> {
    router
        .route("/api/admin/users", get(list_users))
        .route(
            "/api/admin/users/{user_id}",
            get(get_user).delete(delete_user),
        )
        .route(
            "/api/admin/users/{user_id}/status",
            patch(update_user_status),
        )
        .route(
            "/api/admin/users/{user_id}/permissions",
            get(get_user_permissions)
                .post(grant_user_permission)
                .delete(revoke_user_permission),
        )
}
