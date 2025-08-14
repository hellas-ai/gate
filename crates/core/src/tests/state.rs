//! Test harness for StateBackend implementations
//!
//! This module provides a comprehensive test suite that can be used to verify
//! any implementation of the StateBackend trait. Third-party implementations
//! can use this to ensure compliance with the expected behavior.

use crate::{
    ApiKey, Model, ModelType, Organization, Provider, ProviderType, Result, StateBackend,
    TimeRange, UsageRecord, User,
    access::{Action, ObjectIdentity},
};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use std::{collections::HashMap, sync::Arc};

/// Test suite for StateBackend implementations
pub struct StateBackendTestSuite<B: StateBackend> {
    backend: B,
}

impl<B: StateBackend> StateBackendTestSuite<B> {
    /// Create a new test suite with the given backend
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    /// Run all tests
    pub async fn run_all_tests(&self) -> Result<()> {
        self.test_user_operations().await?;
        self.test_api_key_operations().await?;
        self.test_usage_tracking().await?;
        self.test_provider_operations().await?;
        self.test_model_operations().await?;
        self.test_organization_operations().await?;
        Ok(())
    }

    /// Test user CRUD operations
    pub async fn test_user_operations(&self) -> Result<()> {
        // Create test user
        let mut metadata = HashMap::new();
        metadata.insert("email".to_string(), "test@example.com".to_string());
        metadata.insert("name".to_string(), "Test User".to_string());

        let user = User {
            id: format!("test-user-{}", uuid::Uuid::new_v4()),
            name: Some("Test User".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            disabled_at: None,
            metadata: metadata.clone(),
        };

        // Test create
        self.backend.create_user(&user).await?;

        // Test read
        let retrieved = self.backend.get_user_by_id(&user.id).await?;
        assert!(retrieved.is_some(), "User should exist after creation");
        let retrieved_user = retrieved.unwrap();
        assert_eq!(retrieved_user.id, user.id);
        assert_eq!(
            retrieved_user.metadata.get("email"),
            Some(&"test@example.com".to_string())
        );

        // Test update
        let mut updated_user = retrieved_user;
        updated_user.name = Some("Updated Name".to_string());
        updated_user.updated_at = Utc::now();
        self.backend.update_user(&updated_user).await?;

        // Verify update
        let updated = self.backend.get_user_by_id(&user.id).await?.unwrap();
        assert_eq!(updated.name, Some("Updated Name".to_string()));

        // Test non-existent user
        let non_existent = self.backend.get_user_by_id("non-existent").await?;
        assert!(non_existent.is_none());

        Ok(())
    }

    /// Test API key operations
    pub async fn test_api_key_operations(&self) -> Result<()> {
        let org_id = format!("test-org-{}", uuid::Uuid::new_v4());

        // Create organization first
        let org = Organization {
            id: org_id.clone(),
            name: "Test Organization".to_string(),
            created_at: Utc::now(),
            settings: HashMap::new(),
        };
        self.backend.create_organization(&org).await?;

        // Create API key
        let key = ApiKey {
            key_hash: format!("key-hash-{}", uuid::Uuid::new_v4()),
            name: "Test API Key".to_string(),
            org_id: org_id.clone(),
            config: None,
            created_at: Utc::now(),
            last_used_at: None,
        };

        // Test create
        self.backend.create_api_key(&key, "raw-key-value").await?;

        // Test read
        let retrieved = self.backend.get_api_key(&key.key_hash).await?;
        assert!(retrieved.is_some());
        let retrieved_key = retrieved.unwrap();
        assert_eq!(retrieved_key.name, key.name);
        assert_eq!(retrieved_key.org_id, key.org_id);

        // Test list
        let keys = self.backend.list_api_keys(&org_id).await?;
        assert!(!keys.is_empty());
        assert!(keys.iter().any(|k| k.key_hash == key.key_hash));

        // Test delete
        self.backend.delete_api_key(&key.key_hash).await?;
        let deleted = self.backend.get_api_key(&key.key_hash).await?;
        assert!(deleted.is_none());

        Ok(())
    }

    /// Test usage tracking
    pub async fn test_usage_tracking(&self) -> Result<()> {
        let org_id = format!("test-org-{}", uuid::Uuid::new_v4());
        let user_id = format!("test-user-{}", uuid::Uuid::new_v4());

        // Create multiple usage records
        let base_time = Utc::now();
        for i in 0..5 {
            let usage = UsageRecord {
                id: format!("usage-{}", uuid::Uuid::new_v4()),
                org_id: org_id.clone(),
                user_id: user_id.clone(),
                api_key_hash: "test-key-hash".to_string(),
                request_id: format!("req-{i}"),
                provider_id: "openai".to_string(),
                model_id: "gpt-4".to_string(),
                input_tokens: 100 + i as u64 * 10,
                output_tokens: 200 + i as u64 * 20,
                total_tokens: 300 + i as u64 * 30,
                cost: 0.01 * (i + 1) as f64,
                timestamp: base_time - Duration::minutes(i as i64),
                metadata: HashMap::new(),
            };
            self.backend.record_usage(&usage).await?;
        }

        // Test retrieval with time range
        let range = TimeRange {
            start: base_time - Duration::hours(1),
            end: base_time + Duration::hours(1),
        };
        let records = self.backend.get_usage(&org_id, &range).await?;
        assert_eq!(records.len(), 5);

        // Verify ordering (should be descending by timestamp)
        for i in 1..records.len() {
            assert!(records[i - 1].timestamp >= records[i].timestamp);
        }

        // Test empty range
        let empty_range = TimeRange {
            start: base_time - Duration::days(10),
            end: base_time - Duration::days(9),
        };
        let empty_records = self.backend.get_usage(&org_id, &empty_range).await?;
        assert!(empty_records.is_empty());

        Ok(())
    }

    /// Test provider operations
    pub async fn test_provider_operations(&self) -> Result<()> {
        // Note: Providers are typically pre-seeded, so we just test read operations
        // This test assumes the backend starts empty or has a way to seed test data

        // Test list providers (might be empty initially)
        let providers = self.backend.list_providers().await?;

        // If we have providers, test getting one
        if let Some(first_provider) = providers.first() {
            let retrieved = self.backend.get_provider(&first_provider.id).await?;
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().id, first_provider.id);
        }

        // Test non-existent provider
        let non_existent = self.backend.get_provider("non-existent-provider").await?;
        assert!(non_existent.is_none());

        Ok(())
    }

    /// Test model operations
    pub async fn test_model_operations(&self) -> Result<()> {
        // Similar to providers, models are typically pre-seeded
        let models = self.backend.list_models().await?;

        if let Some(first_model) = models.first() {
            let retrieved = self.backend.get_model(&first_model.id).await?;
            assert!(retrieved.is_some());
            assert_eq!(retrieved.unwrap().id, first_model.id);
        }

        // Test non-existent model
        let non_existent = self.backend.get_model("non-existent-model").await?;
        assert!(non_existent.is_none());

        Ok(())
    }

    /// Test organization operations
    pub async fn test_organization_operations(&self) -> Result<()> {
        let mut settings = HashMap::new();
        settings.insert("tier".to_string(), "pro".to_string());
        settings.insert("max_users".to_string(), "100".to_string());

        let org = Organization {
            id: format!("test-org-{}", uuid::Uuid::new_v4()),
            name: "Test Organization".to_string(),
            created_at: Utc::now(),
            settings,
        };

        // Test create
        self.backend.create_organization(&org).await?;

        // Test read
        let retrieved = self.backend.get_organization(&org.id).await?;
        assert!(retrieved.is_some());
        let retrieved_org = retrieved.unwrap();
        assert_eq!(retrieved_org.name, org.name);
        assert_eq!(retrieved_org.settings.get("tier"), Some(&"pro".to_string()));

        // Test non-existent organization
        let non_existent = self.backend.get_organization("non-existent").await?;
        assert!(non_existent.is_none());

        Ok(())
    }
}

/// Helper function to create test data
pub mod fixtures {
    use super::*;

    pub fn create_test_user(id: Option<String>) -> User {
        let mut metadata = HashMap::new();
        metadata.insert("email".to_string(), "test@example.com".to_string());
        metadata.insert("name".to_string(), "Test User".to_string());

        User {
            id: id.unwrap_or_else(|| format!("user-{}", uuid::Uuid::new_v4())),
            name: Some("Test User".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            disabled_at: None,
            metadata,
        }
    }

    pub fn create_test_organization(id: Option<String>) -> Organization {
        Organization {
            id: id.unwrap_or_else(|| format!("org-{}", uuid::Uuid::new_v4())),
            name: "Test Organization".to_string(),
            created_at: Utc::now(),
            settings: HashMap::new(),
        }
    }

    pub fn create_test_api_key(org_id: String) -> ApiKey {
        ApiKey {
            key_hash: format!("key-{}", uuid::Uuid::new_v4()),
            name: "Test API Key".to_string(),
            org_id,
            config: None,
            created_at: Utc::now(),
            last_used_at: None,
        }
    }

    pub fn create_test_provider() -> Provider {
        Provider {
            id: format!("provider-{}", uuid::Uuid::new_v4()),
            name: "Test Provider".to_string(),
            provider_type: ProviderType::OpenAI,
            config: None,
            enabled: true,
            priority: 0,
        }
    }

    pub fn create_test_model(provider_id: String) -> Model {
        Model {
            id: format!("model-{}", uuid::Uuid::new_v4()),
            provider_id,
            name: "test-model".to_string(),
            model_type: ModelType::Chat,
            capabilities: HashMap::new(),
            pricing_id: None,
            pricing_config: None,
        }
    }
}

/// In-memory implementation of StateBackend for integration testing
///
/// This implementation actually stores data and behaves like a real backend,
/// making it suitable for integration tests that need realistic behavior.
///
/// For unit tests that need to verify specific calls and mock errors,
/// use the mockall-based MockStateBackend from the parent module instead.
#[derive(Clone, Default)]
pub struct InMemoryBackend {
    users: Arc<std::sync::Mutex<HashMap<String, User>>>,
    api_keys: Arc<std::sync::Mutex<HashMap<String, ApiKey>>>,
    usage_records: Arc<std::sync::Mutex<Vec<UsageRecord>>>,
    providers: Arc<std::sync::Mutex<HashMap<String, Provider>>>,
    models: Arc<std::sync::Mutex<HashMap<String, Model>>>,
    organizations: Arc<std::sync::Mutex<HashMap<String, Organization>>>,
}

#[async_trait]
impl StateBackend for InMemoryBackend {
    async fn get_user_by_id(&self, user_id: &str) -> Result<Option<User>> {
        Ok(self.users.lock().unwrap().get(user_id).cloned())
    }

    async fn create_user(&self, user: &User) -> Result<()> {
        self.users
            .lock()
            .unwrap()
            .insert(user.id.clone(), user.clone());
        Ok(())
    }

    async fn update_user(&self, user: &User) -> Result<()> {
        self.users
            .lock()
            .unwrap()
            .insert(user.id.clone(), user.clone());
        Ok(())
    }

    async fn get_user(&self, user_id: &str) -> Result<Option<User>> {
        self.get_user_by_id(user_id).await
    }

    async fn delete_user(&self, user_id: &str) -> Result<()> {
        self.users.lock().unwrap().remove(user_id);
        Ok(())
    }

    async fn list_users(&self) -> Result<Vec<User>> {
        let mut results: Vec<_> = self.users.lock().unwrap().values().cloned().collect();
        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(results)
    }

    async fn get_api_key(&self, key_hash: &str) -> Result<Option<ApiKey>> {
        Ok(self.api_keys.lock().unwrap().get(key_hash).cloned())
    }

    async fn create_api_key(&self, key: &ApiKey, _raw_key: &str) -> Result<()> {
        self.api_keys
            .lock()
            .unwrap()
            .insert(key.key_hash.clone(), key.clone());
        Ok(())
    }

    async fn list_api_keys(&self, org_id: &str) -> Result<Vec<ApiKey>> {
        let mut keys: Vec<ApiKey> = self
            .api_keys
            .lock()
            .unwrap()
            .values()
            .filter(|k| k.org_id == org_id)
            .cloned()
            .collect();

        // Sort by created_at descending to match SQL behavior
        keys.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(keys)
    }

    async fn delete_api_key(&self, key_hash: &str) -> Result<()> {
        self.api_keys.lock().unwrap().remove(key_hash);
        Ok(())
    }

    async fn record_usage(&self, usage: &UsageRecord) -> Result<()> {
        self.usage_records.lock().unwrap().push(usage.clone());
        Ok(())
    }

    async fn get_usage(&self, org_id: &str, range: &TimeRange) -> Result<Vec<UsageRecord>> {
        let mut records: Vec<UsageRecord> = self
            .usage_records
            .lock()
            .unwrap()
            .iter()
            .filter(|u| {
                u.org_id == org_id && u.timestamp >= range.start && u.timestamp <= range.end
            })
            .cloned()
            .collect();

        // Sort by timestamp descending to match SQL behavior
        records.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        Ok(records)
    }

    async fn get_provider(&self, id: &str) -> Result<Option<Provider>> {
        Ok(self.providers.lock().unwrap().get(id).cloned())
    }

    async fn list_providers(&self) -> Result<Vec<Provider>> {
        let mut providers: Vec<Provider> = self
            .providers
            .lock()
            .unwrap()
            .values()
            .filter(|p| p.enabled) // Only return enabled providers
            .cloned()
            .collect();

        // Sort by name to match SQL behavior
        providers.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(providers)
    }

    async fn get_model(&self, id: &str) -> Result<Option<Model>> {
        Ok(self.models.lock().unwrap().get(id).cloned())
    }

    async fn list_models(&self) -> Result<Vec<Model>> {
        let mut models: Vec<Model> = self.models.lock().unwrap().values().cloned().collect();

        // Sort by provider_id, then name to match SQL behavior
        models.sort_by(|a, b| match a.provider_id.cmp(&b.provider_id) {
            std::cmp::Ordering::Equal => a.name.cmp(&b.name),
            other => other,
        });

        Ok(models)
    }

    async fn get_organization(&self, id: &str) -> Result<Option<Organization>> {
        Ok(self.organizations.lock().unwrap().get(id).cloned())
    }

    async fn create_organization(&self, org: &Organization) -> Result<()> {
        self.organizations
            .lock()
            .unwrap()
            .insert(org.id.clone(), org.clone());
        Ok(())
    }

    async fn has_permission(
        &self,
        _subject_id: &str,
        _action: &Action,
        _object: &ObjectIdentity,
    ) -> Result<bool> {
        // For tests, just return true
        Ok(true)
    }

    async fn grant_permission(
        &self,
        _subject_id: &str,
        _action: &Action,
        _object: &ObjectIdentity,
    ) -> Result<()> {
        // For tests, just return Ok
        Ok(())
    }

    async fn remove_permission(
        &self,
        _subject_id: &str,
        _action: &Action,
        _object: &ObjectIdentity,
    ) -> Result<()> {
        // For tests, just return Ok
        Ok(())
    }

    async fn list_user_permissions(
        &self,
        _user_id: &str,
    ) -> Result<Vec<(String, String, chrono::DateTime<chrono::Utc>)>> {
        // For tests, return empty list
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_backend_compliance() {
        let backend = InMemoryBackend::default();
        let suite = StateBackendTestSuite::new(backend);
        suite
            .run_all_tests()
            .await
            .expect("InMemoryBackend should pass all tests");
    }

    #[tokio::test]
    async fn test_in_memory_backend_time_filtering() {
        let backend = InMemoryBackend::default();

        // Create some usage records with different timestamps
        let org_id = "test-org";
        let base_time = Utc::now();

        for i in 0..5 {
            let usage = UsageRecord {
                id: format!("usage-{i}"),
                org_id: org_id.to_string(),
                user_id: "user1".to_string(),
                api_key_hash: "key1".to_string(),
                request_id: format!("req-{i}"),
                provider_id: "provider1".to_string(),
                model_id: "model1".to_string(),
                input_tokens: 100,
                output_tokens: 200,
                total_tokens: 300,
                cost: 0.01,
                timestamp: base_time - Duration::hours(i as i64),
                metadata: HashMap::new(),
            };
            backend.record_usage(&usage).await.unwrap();
        }

        // Test range that includes only some records
        let range = TimeRange {
            start: base_time - Duration::hours(2),
            end: base_time,
        };

        let records = backend.get_usage(org_id, &range).await.unwrap();
        assert_eq!(records.len(), 3); // Should include records 0, 1, 2

        // Verify they're sorted by timestamp descending
        for i in 1..records.len() {
            assert!(records[i - 1].timestamp >= records[i].timestamp);
        }
    }

    #[tokio::test]
    async fn test_in_memory_backend_provider_filtering() {
        let backend = InMemoryBackend::default();

        // Insert some providers, some enabled and some disabled
        let providers = vec![
            Provider {
                id: "p1".to_string(),
                name: "Provider B".to_string(),
                provider_type: ProviderType::OpenAI,
                config: None,
                enabled: true,
                priority: 1,
            },
            Provider {
                id: "p2".to_string(),
                name: "Provider A".to_string(),
                provider_type: ProviderType::Anthropic,
                config: None,
                enabled: false, // Disabled
                priority: 2,
            },
            Provider {
                id: "p3".to_string(),
                name: "Provider C".to_string(),
                provider_type: ProviderType::Google,
                config: None,
                enabled: true,
                priority: 3,
            },
        ];

        for provider in providers {
            backend
                .providers
                .lock()
                .unwrap()
                .insert(provider.id.clone(), provider);
        }

        let listed = backend.list_providers().await.unwrap();

        // Should only return enabled providers
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().all(|p| p.enabled));

        // Should be sorted by name
        assert_eq!(listed[0].name, "Provider B");
        assert_eq!(listed[1].name, "Provider C");
    }
}
