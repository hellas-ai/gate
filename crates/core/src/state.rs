use crate::{ApiKey, Model, Organization, Provider, Result, TimeRange, UsageRecord, User};
use async_trait::async_trait;

#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait StateBackend: Send + Sync {
    // User management
    async fn get_user(&self, user_id: &str) -> Result<Option<User>>;
    async fn get_user_by_id(&self, user_id: &str) -> Result<Option<User>>;
    async fn create_user(&self, user: &User) -> Result<()>;
    async fn update_user(&self, user: &User) -> Result<()>;
    async fn delete_user(&self, user_id: &str) -> Result<()>;
    async fn list_users(&self, filter: Option<&str>) -> Result<Vec<User>>;

    // API key management
    async fn get_api_key(&self, key_hash: &str) -> Result<Option<ApiKey>>;
    async fn create_api_key(&self, key: &ApiKey, raw_key: &str) -> Result<()>;
    async fn list_api_keys(&self, org_id: &str) -> Result<Vec<ApiKey>>;
    async fn delete_api_key(&self, key_hash: &str) -> Result<()>;

    // Usage tracking (basic, always visible)
    async fn record_usage(&self, usage: &UsageRecord) -> Result<()>;
    async fn get_usage(&self, org_id: &str, range: &TimeRange) -> Result<Vec<UsageRecord>>;

    // Provider management
    async fn get_provider(&self, id: &str) -> Result<Option<Provider>>;
    async fn list_providers(&self) -> Result<Vec<Provider>>;

    // Model management
    async fn get_model(&self, id: &str) -> Result<Option<Model>>;
    async fn list_models(&self) -> Result<Vec<Model>>;

    // Organization management
    async fn get_organization(&self, id: &str) -> Result<Option<Organization>>;
    async fn create_organization(&self, org: &Organization) -> Result<()>;
}

// Mock implementation for testing
// TODO: Fix mockall lifetime issues with Option<&str> in async functions
// For now, mocks can be created manually in tests as needed
