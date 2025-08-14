//! API Key management endpoints

use crate::{error::HttpError, services::HttpIdentity, state::AppState};
use axum::{
    extract::{Extension, Path, State},
    response::Json,
};
use chrono::Utc;
use gate_core::ApiKey;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tracing::instrument;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

/// Request to create a new API key
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateApiKeyRequest {
    /// Name for the API key
    pub name: String,
    /// Optional configuration for the API key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<JsonValue>,
}

/// Response when creating a new API key
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateApiKeyResponse {
    /// Name of the created API key
    pub name: String,
    /// Hash of the API key (used as identifier)
    pub key_hash: String,
    /// The actual API key (only shown once)
    pub key: String,
    /// Organization ID the key belongs to
    pub org_id: String,
}

/// Request to update an API key
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateApiKeyRequest {
    /// Optional new name for the API key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional new configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<JsonValue>,
}

/// API key response for documentation
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiKeyResponse {
    /// Hash of the API key (used as identifier)
    pub key_hash: String,
    /// Name of the API key
    pub name: String,
    /// Organization ID the key belongs to
    pub org_id: String,
    /// Optional configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<JsonValue>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<Utc>,
    /// Last usage timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<chrono::DateTime<Utc>>,
}

impl From<ApiKey> for ApiKeyResponse {
    fn from(key: ApiKey) -> Self {
        Self {
            key_hash: key.key_hash,
            name: key.name,
            org_id: key.org_id,
            config: key.config,
            created_at: key.created_at,
            last_used_at: key.last_used_at,
        }
    }
}

/// Create a new API key
#[utoipa::path(
    post,
    path = "/api-keys",
    request_body = CreateApiKeyRequest,
    responses(
        (status = 200, description = "API key created successfully", body = CreateApiKeyResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    tag = "api_keys"
)]
#[instrument(
    name = "create_api_key",
    skip(app_state),
    fields(
        user_id = %identity.id,
        key_name = %request.name
    )
)]
pub async fn create_api_key<T>(
    State(app_state): State<AppState<T>>,
    Extension(identity): Extension<HttpIdentity>,
    Json(request): Json<CreateApiKeyRequest>,
) -> Result<Json<CreateApiKeyResponse>, HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    // Generate a new API key
    let key = generate_api_key();
    let key_hash = hash_api_key(&key);

    // Create the API key in the database
    let api_key = ApiKey {
        key_hash: key_hash.clone(),
        name: request.name.clone(),
        org_id: identity.id.clone(),
        config: request.config,
        created_at: Utc::now(),
        last_used_at: None,
    };

    app_state
        .state_backend
        .create_api_key(&api_key, &key)
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;

    Ok(Json(CreateApiKeyResponse {
        name: api_key.name,
        key_hash: api_key.key_hash,
        key,
        org_id: api_key.org_id,
    }))
}

/// List all API keys for the authenticated user's organization
#[utoipa::path(
    get,
    path = "/api-keys",
    responses(
        (status = 200, description = "List of API keys", body = Vec<ApiKeyResponse>),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    ),
    tag = "api_keys"
)]
#[instrument(
    name = "list_api_keys",
    skip(app_state),
    fields(
        user_id = %identity.id,
        key_count = tracing::field::Empty
    )
)]
pub async fn list_api_keys<T>(
    State(app_state): State<AppState<T>>,
    Extension(identity): Extension<HttpIdentity>,
) -> Result<Json<Vec<ApiKeyResponse>>, HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    let keys = app_state
        .state_backend
        .list_api_keys(&identity.id)
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;

    tracing::Span::current().record("key_count", keys.len());

    let response_keys: Vec<ApiKeyResponse> = keys.into_iter().map(Into::into).collect();
    Ok(Json(response_keys))
}

/// Update an API key
#[utoipa::path(
    put,
    path = "/api-keys/{key_hash}",
    params(
        ("key_hash" = String, Path, description = "API key hash")
    ),
    request_body = UpdateApiKeyRequest,
    responses(
        (status = 200, description = "API key updated successfully", body = ApiKeyResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "API key not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "api_keys"
)]
#[instrument(
    name = "update_api_key",
    skip(app_state, request),
    fields(
        user_id = %identity.id,
        key_hash = %key_hash
    )
)]
pub async fn update_api_key<T>(
    State(app_state): State<AppState<T>>,
    Extension(identity): Extension<HttpIdentity>,
    Path(key_hash): Path<String>,
    Json(request): Json<UpdateApiKeyRequest>,
) -> Result<Json<ApiKeyResponse>, HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    // Get the existing API key
    let mut api_key = app_state
        .state_backend
        .get_api_key(&key_hash)
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?
        .ok_or(HttpError::NotFound("API key not found".to_string()))?;

    // Verify ownership
    if api_key.org_id != identity.id {
        return Err(HttpError::AuthorizationFailed("Not authorized".to_string()));
    }

    // Update fields if provided
    if let Some(name) = request.name {
        api_key.name = name;
    }
    if let Some(config) = request.config {
        api_key.config = Some(config);
    }

    // Note: The actual update would require an update_api_key method in StateBackend
    // For now, we'll just return the updated key
    // TODO: Add update_api_key to StateBackend trait

    Ok(Json(api_key.into()))
}

/// Delete an API key
#[utoipa::path(
    delete,
    path = "/api-keys/{key_hash}",
    params(
        ("key_hash" = String, Path, description = "API key hash")
    ),
    responses(
        (status = 204, description = "API key deleted successfully"),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "API key not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "api_keys"
)]
#[instrument(
    name = "delete_api_key",
    skip(app_state),
    fields(
        user_id = %identity.id,
        key_hash = %key_hash
    )
)]
pub async fn delete_api_key<T>(
    State(app_state): State<AppState<T>>,
    Extension(identity): Extension<HttpIdentity>,
    Path(key_hash): Path<String>,
) -> Result<(), HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    // Get the API key to verify ownership
    let api_key = app_state
        .state_backend
        .get_api_key(&key_hash)
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?
        .ok_or(HttpError::NotFound("API key not found".to_string()))?;

    // Verify ownership
    if api_key.org_id != identity.id {
        return Err(HttpError::AuthorizationFailed("Not authorized".to_string()));
    }

    // Delete the key
    app_state
        .state_backend
        .delete_api_key(&key_hash)
        .await
        .map_err(|e| HttpError::InternalServerError(e.to_string()))?;

    Ok(())
}

/// Generate a new API key
fn generate_api_key() -> String {
    use rand::{Rng, distributions::Alphanumeric};

    let key: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    format!("gk_{key}")
}

/// Hash an API key using SHA-256
fn hash_api_key(key: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let result = hasher.finalize();

    format!("{result:x}")
}

/// Create the API keys router
pub fn router<T: Send + Sync + Clone + 'static>() -> OpenApiRouter<AppState<T>> {
    // Add OpenAI routes
    OpenApiRouter::new()
        .routes(routes!(create_api_key))
        .routes(routes!(list_api_keys))
        .routes(routes!(update_api_key))
        .routes(routes!(delete_api_key))
}
