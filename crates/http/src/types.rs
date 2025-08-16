//! Common types used by both client and server

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Anthropic Messages request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct AnthropicMessagesRequest {
    pub model: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(flatten)]
    pub extra: Option<JsonValue>,
}

/// OpenAI Chat Completion request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct OpenAIChatCompletionRequest {
    pub model: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(flatten)]
    pub extra: JsonValue,
}

/// OpenAI Completion request (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct OpenAICompletionRequest {
    pub model: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(flatten)]
    pub extra: Option<JsonValue>,
}

/// Registration start request
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct RegisterStartRequest {
    /// Display name for the account
    pub name: String,
}

/// Registration start response
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct RegisterStartResponse {
    /// WebAuthn challenge data (as JSON)
    pub challenge: JsonValue,
    /// Session ID to track registration
    pub session_id: String,
}

/// Registration complete request
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct RegisterCompleteRequest {
    /// Session ID from registration start
    pub session_id: String,
    /// WebAuthn credential response (as JSON)
    pub credential: JsonValue,
    /// Optional device name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    /// Optional bootstrap token (required for first user)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bootstrap_token: Option<String>,
}

/// Registration complete response
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct RegisterCompleteResponse {
    /// User ID (same as credential ID)
    pub user_id: String,
    /// Display name
    pub name: String,
    /// Credential ID
    pub credential_id: String,
    /// JWT token for authenticated session
    pub token: String,
}

/// Authentication start response
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct AuthStartResponse {
    /// WebAuthn challenge data (as JSON)
    pub challenge: JsonValue,
    /// Session ID to track authentication
    pub session_id: String,
}

/// Authentication complete request
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct AuthCompleteRequest {
    /// Session ID from auth start
    pub session_id: String,
    /// WebAuthn credential response (as JSON)
    pub credential: JsonValue,
}

/// Authentication complete response
#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct AuthCompleteResponse {
    /// User ID
    pub user_id: String,
    /// Display name
    pub name: String,
    /// JWT token for authenticated session
    pub token: String,
}

/// Configuration response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct ConfigResponse {
    /// The configuration data as JSON
    pub config: JsonValue,
}

/// Configuration update request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct ConfigUpdateRequest {
    /// The new configuration data as JSON
    pub config: JsonValue,
}

/// Configuration patch request for updating specific paths
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct ConfigPatchRequest {
    /// The value to set at the specified path
    pub value: JsonValue,
}

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct HealthCheckResponse {
    /// Health status (e.g., "healthy", "degraded", "unhealthy")
    pub status: String,
    /// ISO 8601 timestamp
    pub timestamp: String,
    /// Service name
    pub service: String,
    /// Service version
    pub version: String,
}

/// Model information in OpenAI format
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct ModelInfo {
    /// Model identifier
    pub id: String,
    /// Object type (always "model")
    pub object: String,
    /// Owner of the model
    pub owned_by: String,
    /// Unix timestamp of when the model was created
    pub created: i64,
    /// Optional context length for local models
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length: Option<usize>,
}

/// OpenAI-compatible models list response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(utoipa::ToSchema))]
pub struct ModelsListResponse {
    /// Object type (always "list")
    pub object: String,
    /// List of available models
    pub data: Vec<ModelInfo>,
}
