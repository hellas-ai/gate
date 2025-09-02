use crate::{
    ApiKey, Model, Organization, Provider, Result, TimeRange, UsageRecord, User,
    access::{Action, ObjectIdentity},
};
use async_trait::async_trait;
// serde traits not needed in this module now

#[async_trait]
pub trait StateBackend: Send + Sync {
    // User management
    async fn get_user(&self, user_id: &str) -> Result<Option<User>>;
    async fn get_user_by_id(&self, user_id: &str) -> Result<Option<User>>;
    async fn create_user(&self, user: &User) -> Result<()>;
    async fn update_user(&self, user: &User) -> Result<()>;
    async fn delete_user(&self, user_id: &str) -> Result<()>;
    async fn list_users(&self) -> Result<Vec<User>>;

    // API key management
    async fn get_api_key(&self, key_hash: &str) -> Result<Option<ApiKey>>;
    async fn create_api_key(&self, key: &ApiKey, raw_key: &str) -> Result<()>;
    async fn list_api_keys(&self, org_id: &str) -> Result<Vec<ApiKey>>;
    async fn delete_api_key(&self, key_hash: &str) -> Result<()>;

    // Usage tracking
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

    // Permission management
    async fn has_permission(
        &self,
        subject_id: &str,
        action: &Action,
        object: &ObjectIdentity,
    ) -> Result<bool>;

    async fn grant_permission(
        &self,
        subject_id: &str,
        action: &Action,
        object: &ObjectIdentity,
    ) -> Result<()>;

    async fn remove_permission(
        &self,
        subject_id: &str,
        action: &Action,
        object: &ObjectIdentity,
    ) -> Result<()>;

    async fn list_user_permissions(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, String, chrono::DateTime<chrono::Utc>)>>;

    // Router-specific methods with default implementations
    async fn resolve_model_alias(&self, _alias: &str) -> Result<Vec<String>> {
        Err(crate::Error::Internal(
            "Model alias resolution not implemented".into(),
        ))
    }

    async fn get_virtual_model(
        &self,
        _name: &str,
        _version: Option<&str>,
    ) -> Result<Option<crate::router::types::VirtualModel>> {
        Err(crate::Error::Internal(
            "Virtual models not implemented".into(),
        ))
    }

    async fn save_virtual_model(&self, _model: &crate::router::types::VirtualModel) -> Result<()> {
        Err(crate::Error::Internal(
            "Virtual models not implemented".into(),
        ))
    }

    async fn update_sink_health(
        &self,
        _sink_id: &str,
        _health: &crate::router::types::SinkHealth,
    ) -> Result<()> {
        Err(crate::Error::Internal(
            "Sink health tracking not implemented".into(),
        ))
    }

    async fn get_sink_health(
        &self,
        _sink_id: &str,
    ) -> Result<Option<crate::router::types::SinkHealth>> {
        Err(crate::Error::Internal(
            "Sink health tracking not implemented".into(),
        ))
    }
}

// Router-specific types moved to crate::router::types
