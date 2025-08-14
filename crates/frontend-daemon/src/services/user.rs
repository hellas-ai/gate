//! User management service

use gate_frontend_common::client::{create_authenticated_client, ClientError};
use reqwest::Method;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserInfo {
    pub id: String,
    pub name: Option<String>,
    pub enabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub disabled_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserListResponse {
    pub users: Vec<UserInfo>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserStatusRequest {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserStatusResponse {
    pub user: UserInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPermission {
    pub action: String,
    pub object: String,
    pub granted_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPermissionsResponse {
    pub permissions: Vec<UserPermission>,
}

#[derive(Clone)]
pub struct UserService;

impl UserService {
    pub fn new() -> Self {
        Self
    }

    /// List all users with pagination
    pub async fn list_users(
        &self,
        page: usize,
        page_size: usize,
        search: Option<String>,
    ) -> Result<UserListResponse, ClientError> {
        let client = create_authenticated_client()?
            .ok_or_else(|| ClientError::Configuration("Not authenticated".into()))?;

        let mut query_params = vec![
            ("page", page.to_string()),
            ("page_size", page_size.to_string()),
        ];

        if let Some(search_term) = search {
            query_params.push(("search", search_term));
        }

        let response: UserListResponse = client
            .execute(
                client
                    .request(Method::GET, "/api/admin/users")
                    .query(&query_params),
            )
            .await?;

        Ok(response)
    }

    /// Get a specific user's details
    pub async fn get_user(&self, user_id: &str) -> Result<UserInfo, ClientError> {
        let client = create_authenticated_client()?
            .ok_or_else(|| ClientError::Configuration("Not authenticated".into()))?;

        let response: UserInfo = client
            .execute(client.request(Method::GET, &format!("/api/admin/users/{user_id}")))
            .await?;

        Ok(response)
    }

    /// Update user status (enable/disable)
    pub async fn update_user_status(
        &self,
        user_id: &str,
        enabled: bool,
    ) -> Result<UserInfo, ClientError> {
        let client = create_authenticated_client()?
            .ok_or_else(|| ClientError::Configuration("Not authenticated".into()))?;

        let request_body = UpdateUserStatusRequest { enabled };

        let response: UpdateUserStatusResponse = client
            .execute(
                client
                    .request(Method::PATCH, &format!("/api/admin/users/{user_id}/status"))
                    .json(&request_body),
            )
            .await?;

        Ok(response.user)
    }

    /// Delete a user
    pub async fn delete_user(&self, user_id: &str) -> Result<(), ClientError> {
        let client = create_authenticated_client()?
            .ok_or_else(|| ClientError::Configuration("Not authenticated".into()))?;

        // For DELETE with no response body, we expect empty JSON
        let _: serde_json::Value = client
            .execute(client.request(Method::DELETE, &format!("/api/admin/users/{user_id}")))
            .await?;

        Ok(())
    }

    /// Get user's permissions
    pub async fn get_user_permissions(
        &self,
        user_id: &str,
    ) -> Result<Vec<UserPermission>, ClientError> {
        let client = create_authenticated_client()?
            .ok_or_else(|| ClientError::Configuration("Not authenticated".into()))?;

        let response: UserPermissionsResponse = client
            .execute(client.request(
                Method::GET,
                &format!("/api/admin/users/{user_id}/permissions"),
            ))
            .await?;

        Ok(response.permissions)
    }

    /// Grant permission to user
    pub async fn grant_permission(
        &self,
        user_id: &str,
        action: &str,
        object: &str,
    ) -> Result<(), ClientError> {
        let client = create_authenticated_client()?
            .ok_or_else(|| ClientError::Configuration("Not authenticated".into()))?;

        #[derive(Serialize)]
        struct GrantRequest {
            action: String,
            object: String,
        }

        let request_body = GrantRequest {
            action: action.to_string(),
            object: object.to_string(),
        };

        let _: serde_json::Value = client
            .execute(
                client
                    .request(
                        Method::POST,
                        &format!("/api/admin/users/{user_id}/permissions"),
                    )
                    .json(&request_body),
            )
            .await?;

        Ok(())
    }

    /// Revoke permission from user
    pub async fn revoke_permission(
        &self,
        user_id: &str,
        action: &str,
        object: &str,
    ) -> Result<(), ClientError> {
        let client = create_authenticated_client()?
            .ok_or_else(|| ClientError::Configuration("Not authenticated".into()))?;

        let query_params = vec![("action", action), ("object", object)];

        let _: serde_json::Value = client
            .execute(
                client
                    .request(
                        Method::DELETE,
                        &format!("/api/admin/users/{user_id}/permissions"),
                    )
                    .query(&query_params),
            )
            .await?;

        Ok(())
    }
}

impl Default for UserService {
    fn default() -> Self {
        Self::new()
    }
}
