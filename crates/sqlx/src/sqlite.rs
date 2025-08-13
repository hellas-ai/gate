use crate::common::{
    ApiKeyRow, ModelRow, OrganizationRow, ProviderRow, UsageRecordRow, UserRow, datetime_to_string,
};
use async_trait::async_trait;
use gate_core::{
    ApiKey, Error, Model, Organization, Provider, Result, StateBackend, TimeRange, UsageRecord,
    User,
    access::{Action, ObjectIdentity},
};
use sqlx::{Pool, Sqlite};

pub struct SqliteStateBackend {
    pool: Pool<Sqlite>,
}

impl SqliteStateBackend {
    pub async fn new(database_url: &str) -> Result<Self> {
        use sqlx::sqlite::SqliteConnectOptions;
        use std::str::FromStr;

        let options = SqliteConnectOptions::from_str(database_url)
            .map_err(|e| Error::StateError(format!("Invalid database URL: {e}")))?
            .create_if_missing(true);

        let pool = sqlx::SqlitePool::connect_with(options)
            .await
            .map_err(|e| Error::StateError(format!("Failed to connect to database: {e}")))?;

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|e| Error::StateError(format!("Failed to run migrations: {e}")))?;

        Ok(Self { pool })
    }

    pub fn from_pool(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }
}

#[async_trait]
impl StateBackend for SqliteStateBackend {
    // User management
    async fn get_user(&self, user_id: &str) -> Result<Option<User>> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, name, created_at, updated_at FROM users WHERE id = ?1",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to get user: {e}")))?;

        Ok(row.map(User::from))
    }

    async fn get_user_by_id(&self, user_id: &str) -> Result<Option<User>> {
        self.get_user(user_id).await
    }

    async fn create_user(&self, user: &User) -> Result<()> {
        let email = user.metadata.get("email").map(|s| s.as_str());
        let created_at = datetime_to_string(user.created_at);
        let updated_at = datetime_to_string(user.updated_at);

        sqlx::query(
            "INSERT INTO users (id, email, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(&user.id)
        .bind(email)
        .bind(&user.name)
        .bind(&created_at)
        .bind(&updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to create user: {e}")))?;

        Ok(())
    }

    async fn update_user(&self, user: &User) -> Result<()> {
        let email = user.metadata.get("email").map(|s| s.as_str());
        let updated_at = datetime_to_string(user.updated_at);

        sqlx::query("UPDATE users SET email = ?2, name = ?3, updated_at = ?4 WHERE id = ?1")
            .bind(&user.id)
            .bind(email)
            .bind(&user.name)
            .bind(&updated_at)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::StateError(format!("Failed to update user: {e}")))?;

        Ok(())
    }

    async fn delete_user(&self, user_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM users WHERE id = ?1")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::StateError(format!("Failed to delete user: {e}")))?;

        Ok(())
    }

    async fn list_users(&self) -> Result<Vec<User>> {
        let rows = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, name, created_at, updated_at FROM users ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to list users: {e}")))?;

        Ok(rows.into_iter().map(User::from).collect())
    }

    // API key management
    async fn get_api_key(&self, key_hash: &str) -> Result<Option<ApiKey>> {
        let row = sqlx::query_as::<_, ApiKeyRow>(
            "SELECT key_hash, name, org_id, config, created_at, last_used_at FROM api_keys WHERE key_hash = ?1",
        )
        .bind(key_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to get API key: {e}")))?;

        Ok(row.map(ApiKey::from))
    }

    async fn create_api_key(&self, key: &ApiKey, _raw_key: &str) -> Result<()> {
        let config = key
            .config
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| Error::StateError(format!("Failed to serialize config: {e}")))?;
        let created_at = datetime_to_string(key.created_at);
        let last_used_at = key.last_used_at.map(datetime_to_string);

        sqlx::query(
            "INSERT INTO api_keys (key_hash, name, org_id, config, created_at, last_used_at) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(&key.key_hash)
        .bind(&key.name)
        .bind(&key.org_id)
        .bind(config)
        .bind(&created_at)
        .bind(last_used_at)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to create API key: {e}")))?;

        Ok(())
    }

    async fn list_api_keys(&self, org_id: &str) -> Result<Vec<ApiKey>> {
        let rows = sqlx::query_as::<_, ApiKeyRow>(
            "SELECT key_hash, name, org_id, config, created_at, last_used_at 
             FROM api_keys WHERE org_id = ?1 ORDER BY created_at DESC",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to list API keys: {e}")))?;

        Ok(rows.into_iter().map(ApiKey::from).collect())
    }

    async fn delete_api_key(&self, key_hash: &str) -> Result<()> {
        sqlx::query("DELETE FROM api_keys WHERE key_hash = ?1")
            .bind(key_hash)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::StateError(format!("Failed to delete API key: {e}")))?;

        Ok(())
    }

    // Usage tracking
    async fn record_usage(&self, usage: &UsageRecord) -> Result<()> {
        let metadata = serde_json::to_string(&usage.metadata)
            .map_err(|e| Error::StateError(format!("Failed to serialize metadata: {e}")))?;
        let timestamp = datetime_to_string(usage.timestamp);

        sqlx::query(
            "INSERT INTO usage_records 
             (id, org_id, user_id, api_key_hash, request_id, provider_id, model_id, 
              input_tokens, output_tokens, total_tokens, cost, timestamp, metadata) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        )
        .bind(&usage.id)
        .bind(&usage.org_id)
        .bind(&usage.user_id)
        .bind(&usage.api_key_hash)
        .bind(&usage.request_id)
        .bind(&usage.provider_id)
        .bind(&usage.model_id)
        .bind(usage.input_tokens as i64)
        .bind(usage.output_tokens as i64)
        .bind(usage.total_tokens as i64)
        .bind(usage.cost)
        .bind(&timestamp)
        .bind(&metadata)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to record usage: {e}")))?;

        Ok(())
    }

    async fn get_usage(&self, org_id: &str, range: &TimeRange) -> Result<Vec<UsageRecord>> {
        let start = datetime_to_string(range.start);
        let end = datetime_to_string(range.end);

        let rows = sqlx::query_as::<_, UsageRecordRow>(
            "SELECT id, org_id, user_id, api_key_hash, request_id, provider_id, model_id, 
             input_tokens, output_tokens, total_tokens, cost, timestamp, metadata 
             FROM usage_records WHERE org_id = ?1 AND timestamp >= ?2 AND timestamp <= ?3 
             ORDER BY timestamp DESC",
        )
        .bind(org_id)
        .bind(&start)
        .bind(&end)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to get usage: {e}")))?;

        Ok(rows.into_iter().map(UsageRecord::from).collect())
    }

    // Provider management
    async fn get_provider(&self, id: &str) -> Result<Option<Provider>> {
        let row = sqlx::query_as::<_, ProviderRow>(
            "SELECT id, name, provider_type, config, enabled, priority FROM providers WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to get provider: {e}")))?;

        Ok(row.map(Provider::from))
    }

    async fn list_providers(&self) -> Result<Vec<Provider>> {
        let rows = sqlx::query_as::<_, ProviderRow>(
            "SELECT id, name, provider_type, config, enabled, priority 
             FROM providers ORDER BY priority ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to list providers: {e}")))?;

        Ok(rows.into_iter().map(Provider::from).collect())
    }

    // Model management
    async fn get_model(&self, id: &str) -> Result<Option<Model>> {
        let row = sqlx::query_as::<_, ModelRow>(
            "SELECT id, provider_id, name, model_type, capabilities, pricing_id, pricing_config 
             FROM models WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to get model: {e}")))?;

        Ok(row.map(Model::from))
    }

    async fn list_models(&self) -> Result<Vec<Model>> {
        let rows = sqlx::query_as::<_, ModelRow>(
            "SELECT id, provider_id, name, model_type, capabilities, pricing_id, pricing_config 
             FROM models ORDER BY provider_id, name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to list models: {e}")))?;

        Ok(rows.into_iter().map(Model::from).collect())
    }

    // Organization management
    async fn get_organization(&self, id: &str) -> Result<Option<Organization>> {
        let row = sqlx::query_as::<_, OrganizationRow>(
            "SELECT id, name, created_at, settings FROM organizations WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to get organization: {e}")))?;

        Ok(row.map(Organization::from))
    }

    async fn create_organization(&self, org: &Organization) -> Result<()> {
        let settings = serde_json::to_string(&org.settings)
            .map_err(|e| Error::StateError(format!("Failed to serialize settings: {e}")))?;
        let created_at = datetime_to_string(org.created_at);

        sqlx::query(
            "INSERT INTO organizations (id, name, created_at, settings) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(&org.id)
        .bind(&org.name)
        .bind(&created_at)
        .bind(&settings)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to create organization: {e}")))?;

        Ok(())
    }

    // Permission management
    async fn has_permission(
        &self,
        subject_id: &str,
        action: &Action,
        object: &ObjectIdentity,
    ) -> Result<bool> {
        let action_str = serde_json::to_string(action)
            .map_err(|e| Error::StateError(format!("Failed to serialize action: {e}")))?;
        let object_str = format!("{object}");

        let result: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM permissions
            WHERE subject_id = ?1 AND action = ?2 AND object = ?3
            "#,
        )
        .bind(subject_id)
        .bind(action_str)
        .bind(object_str)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to check permission: {e}")))?;

        Ok(result.0 > 0)
    }

    async fn grant_permission(
        &self,
        subject_id: &str,
        action: &Action,
        object: &ObjectIdentity,
    ) -> Result<()> {
        let action_str = serde_json::to_string(action)
            .map_err(|e| Error::StateError(format!("Failed to serialize action: {e}")))?;
        let object_str = format!("{object}");

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO permissions (subject_id, action, object, granted_at)
            VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
            "#,
        )
        .bind(subject_id)
        .bind(action_str)
        .bind(object_str)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to grant permission: {e}")))?;

        Ok(())
    }

    async fn remove_permission(
        &self,
        subject_id: &str,
        action: &Action,
        object: &ObjectIdentity,
    ) -> Result<()> {
        let action_str = serde_json::to_string(action)
            .map_err(|e| Error::StateError(format!("Failed to serialize action: {e}")))?;
        let object_str = format!("{object}");

        sqlx::query(
            r#"
            DELETE FROM permissions
            WHERE subject_id = ?1 AND action = ?2 AND object = ?3
            "#,
        )
        .bind(subject_id)
        .bind(action_str)
        .bind(object_str)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to remove permission: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gate_core::tests::state::StateBackendTestSuite;

    async fn setup_sqlite_backend() -> SqliteStateBackend {
        SqliteStateBackend::new(":memory:").await.unwrap()
    }

    #[tokio::test]
    async fn test_sqlite_compliance() {
        let backend = setup_sqlite_backend().await;
        let suite = StateBackendTestSuite::new(backend);
        suite.run_all_tests().await.expect("All tests should pass");
    }
}
