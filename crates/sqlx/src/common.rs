//! Common types and utilities shared between database implementations

use chrono::{DateTime, Utc};
use gate_core::{
    ApiKey, Error, Model, ModelType, Organization, Provider, ProviderType, Result, UsageRecord,
    User,
};
use sqlx::FromRow;
use std::collections::HashMap;

// Helper functions for timestamp conversion
pub fn datetime_to_string(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

pub fn string_to_datetime(s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| Error::StateError(format!("Invalid timestamp format: {e}")))
}

// Database row types with FromRow derives
#[derive(FromRow)]
pub struct UserRow {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub created_at: String,          // ISO8601 format
    pub updated_at: String,          // ISO8601 format
    pub disabled_at: Option<String>, // ISO8601 format
}

#[derive(FromRow)]
pub struct ApiKeyRow {
    pub key_hash: String,
    pub name: String,
    pub org_id: String,
    pub config: Option<String>,       // JSON string
    pub created_at: String,           // ISO8601 format
    pub last_used_at: Option<String>, // ISO8601 format
}

#[derive(FromRow)]
pub struct UsageRecordRow {
    pub id: String,
    pub org_id: String,
    pub user_id: String,
    pub api_key_hash: String,
    pub request_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cost: f64,
    pub timestamp: String,        // ISO8601 format
    pub metadata: Option<String>, // JSON string
}

#[derive(FromRow)]
pub struct ProviderRow {
    pub id: String,
    pub name: String,
    pub provider_type: String,  // Store as string for portability
    pub config: Option<String>, // JSON string
    pub enabled: i32,           // SQLite uses INTEGER for boolean
    pub priority: i32,
}

#[derive(FromRow)]
pub struct ModelRow {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub model_type: String,           // Store as string for portability
    pub capabilities: Option<String>, // JSON string
    pub pricing_id: Option<String>,
    pub pricing_config: Option<String>, // JSON string
}

#[derive(FromRow)]
pub struct OrganizationRow {
    pub id: String,
    pub name: String,
    pub created_at: String,       // ISO8601 format
    pub settings: Option<String>, // JSON string
}

// Conversion implementations
impl From<UserRow> for User {
    fn from(row: UserRow) -> Self {
        let mut metadata = HashMap::new();
        if let Some(email) = row.email {
            metadata.insert("email".to_string(), email);
        }

        User {
            id: row.id,
            name: row.name,
            created_at: string_to_datetime(&row.created_at).unwrap_or_else(|_| Utc::now()),
            updated_at: string_to_datetime(&row.updated_at).unwrap_or_else(|_| Utc::now()),
            disabled_at: row.disabled_at.and_then(|s| string_to_datetime(&s).ok()),
            metadata,
        }
    }
}

impl From<ApiKeyRow> for ApiKey {
    fn from(row: ApiKeyRow) -> Self {
        ApiKey {
            key_hash: row.key_hash,
            name: row.name,
            org_id: row.org_id,
            config: row.config.and_then(|json| serde_json::from_str(&json).ok()),
            created_at: string_to_datetime(&row.created_at).unwrap_or_else(|_| Utc::now()),
            last_used_at: row.last_used_at.and_then(|s| string_to_datetime(&s).ok()),
        }
    }
}

impl From<UsageRecordRow> for UsageRecord {
    fn from(row: UsageRecordRow) -> Self {
        let metadata = row
            .metadata
            .and_then(|json| serde_json::from_str::<HashMap<String, String>>(&json).ok())
            .unwrap_or_default();

        UsageRecord {
            id: row.id,
            org_id: row.org_id,
            user_id: row.user_id,
            api_key_hash: row.api_key_hash,
            request_id: row.request_id,
            provider_id: row.provider_id,
            model_id: row.model_id,
            input_tokens: row.input_tokens as u64,
            output_tokens: row.output_tokens as u64,
            total_tokens: row.total_tokens as u64,
            cost: row.cost,
            timestamp: string_to_datetime(&row.timestamp).unwrap_or_else(|_| Utc::now()),
            metadata,
        }
    }
}

impl From<ProviderRow> for Provider {
    fn from(row: ProviderRow) -> Self {
        Provider {
            id: row.id,
            name: row.name,
            provider_type: match row.provider_type.as_str() {
                "openai" => ProviderType::OpenAI,
                "anthropic" => ProviderType::Anthropic,
                "google" => ProviderType::Google,
                "local" => ProviderType::Local,
                "custom" => ProviderType::Custom,
                _ => ProviderType::Unknown,
            },
            config: row.config.and_then(|json| serde_json::from_str(&json).ok()),
            enabled: row.enabled != 0,
            priority: row.priority as u32,
        }
    }
}

impl From<ModelRow> for Model {
    fn from(row: ModelRow) -> Self {
        let capabilities = row
            .capabilities
            .and_then(|json| serde_json::from_str::<HashMap<String, String>>(&json).ok())
            .unwrap_or_default();

        Model {
            id: row.id,
            provider_id: row.provider_id,
            name: row.name,
            model_type: match row.model_type.as_str() {
                "chat" => ModelType::Chat,
                "completion" => ModelType::Completion,
                "embedding" => ModelType::Embedding,
                "image" => ModelType::Image,
                "audio" => ModelType::Audio,
                _ => ModelType::Unknown,
            },
            capabilities,
            pricing_id: row.pricing_id,
            pricing_config: row
                .pricing_config
                .and_then(|json| serde_json::from_str(&json).ok()),
        }
    }
}

impl From<OrganizationRow> for Organization {
    fn from(row: OrganizationRow) -> Self {
        let settings = row
            .settings
            .and_then(|json| serde_json::from_str::<HashMap<String, String>>(&json).ok())
            .unwrap_or_default();

        Organization {
            id: row.id,
            name: row.name,
            created_at: string_to_datetime(&row.created_at).unwrap_or_else(|_| Utc::now()),
            settings,
        }
    }
}
