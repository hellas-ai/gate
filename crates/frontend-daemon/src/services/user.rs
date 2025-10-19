//! User management service

use gate_frontend_common::ClientError;
use gate_http::client::AuthenticatedGateClient;
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
pub struct UserActivity {
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
    pub total_requests: u64,
    pub recent_activity: Vec<ActivityEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub endpoint: String,
    pub status_code: u16,
}

/// User management service
#[derive(Clone)]
pub struct UserService {
    client: AuthenticatedGateClient,
}

impl UserService {
    pub fn new(client: AuthenticatedGateClient) -> Self {
        Self { client }
    }

    /// List all users with pagination
    pub async fn list_users(
        &self,
        page: usize,
        page_size: usize,
        search: Option<String>,
    ) -> Result<UserListResponse, ClientError> {
        let mut query_params = vec![
            ("page", page.to_string()),
            ("page_size", page_size.to_string()),
        ];

        if let Some(search_term) = search {
            query_params.push(("search", search_term));
        }

        let response: UserListResponse = self
            .client
            .execute(
                self.client
                    .request(Method::GET, "/api/admin/users")?
                    .query(&query_params),
            )
            .await?;

        Ok(response)
    }

    /// Get a specific user's details
    pub async fn get_user(&self, user_id: &str) -> Result<UserInfo, ClientError> {
        let path = format!("/api/admin/users/{user_id}");
        let response: UserInfo = self
            .client
            .execute(self.client.request(Method::GET, &path)?)
            .await?;

        Ok(response)
    }

    /// Update a user's status (enable/disable)
    pub async fn update_user_status(
        &self,
        user_id: &str,
        enabled: bool,
    ) -> Result<UserInfo, ClientError> {
        let path = format!("/api/admin/users/{user_id}/status");
        let response: UserInfo = self
            .client
            .execute(
                self.client
                    .request(Method::PUT, &path)?
                    .json(&UpdateUserStatusRequest { enabled }),
            )
            .await?;

        Ok(response)
    }

    /// Delete a user
    pub async fn delete_user(&self, user_id: &str) -> Result<(), ClientError> {
        let path = format!("/api/admin/users/{user_id}");
        let _: serde_json::Value = self
            .client
            .execute(self.client.request(Method::DELETE, &path)?)
            .await?;

        Ok(())
    }

    /// Revoke all of a user's active sessions
    pub async fn revoke_user_sessions(&self, user_id: &str) -> Result<(), ClientError> {
        let path = format!("/api/admin/users/{user_id}/sessions");
        let _: serde_json::Value = self
            .client
            .execute(self.client.request(Method::DELETE, &path)?)
            .await?;

        Ok(())
    }

    /// Revoke all of a user's API keys
    pub async fn revoke_user_api_keys(&self, user_id: &str) -> Result<(), ClientError> {
        let path = format!("/api/admin/users/{user_id}/api-keys");
        let _: serde_json::Value = self
            .client
            .execute(self.client.request(Method::DELETE, &path)?)
            .await?;

        Ok(())
    }

    /// Get user activity
    pub async fn get_user_activity(&self, user_id: &str) -> Result<UserActivity, ClientError> {
        let path = format!("/api/admin/users/{user_id}/activity");
        let response: UserActivity = self
            .client
            .execute(self.client.request(Method::GET, &path)?)
            .await?;

        Ok(response)
    }

    /// Get aggregated user statistics
    pub async fn get_user_stats(&self) -> Result<UserStats, ClientError> {
        let response: UserStats = self
            .client
            .execute(self.client.request(Method::GET, "/api/admin/users/stats")?)
            .await?;

        Ok(response)
    }
}

impl Default for UserService {
    fn default() -> Self {
        panic!("UserService requires a client - use UserService::new(client)")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStats {
    pub total_users: usize,
    pub active_users: usize,
    pub disabled_users: usize,
    pub users_created_today: usize,
    pub users_created_this_week: usize,
    pub users_created_this_month: usize,
}
