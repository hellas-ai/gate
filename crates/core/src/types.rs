use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub disabled_at: Option<DateTime<Utc>>,
    pub metadata: HashMap<String, String>,
}

impl User {
    /// Check if the user is enabled (disabled_at is None)
    pub fn is_enabled(&self) -> bool {
        self.disabled_at.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub settings: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub key_hash: String,
    pub name: String,
    pub org_id: String,
    pub config: Option<JsonValue>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum ProviderType {
    Unknown,
    OpenAI,
    Anthropic,
    Google,
    Local,
    Custom,
}

impl Default for ProviderType {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub config: Option<JsonValue>,
    pub enabled: bool,
    pub priority: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum ModelType {
    Unknown,
    Chat,
    Completion,
    Embedding,
    Image,
    Audio,
}

impl Default for ModelType {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub model_type: ModelType,
    pub capabilities: HashMap<String, String>,
    pub pricing_id: Option<String>,
    pub pricing_config: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub id: String,
    pub org_id: String,
    pub user_id: String,
    pub api_key_hash: String,
    pub request_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub cost: f64,
    pub timestamp: DateTime<Utc>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    pub code: String,
    pub message: String,
    pub details: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub body: JsonValue,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub body: JsonValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default)]
    pub is_streaming: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesRequest {
    pub model: String,
    pub body: JsonValue,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagesResponse {
    pub body: JsonValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default)]
    pub is_streaming: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookAction {
    pub action_type: String,
    pub payload: Option<JsonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookResponse {
    pub actions: Vec<HookAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestHookContext {
    pub request_id: String,
    pub user: Option<User>,
    pub organization: Option<Organization>,
    pub api_key: Option<ApiKey>,
    pub request: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseHookContext {
    pub request_id: String,
    pub user: Option<User>,
    pub organization: Option<Organization>,
    pub api_key: Option<ApiKey>,
    pub request: JsonValue,
    pub response: JsonValue,
    pub usage: Option<UsageRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingChatCompletionResponse {
    pub chunk: JsonValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default)]
    pub is_final: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingMessagesResponse {
    pub chunk: JsonValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default)]
    pub is_final: bool,
}
