//! Anthropic-specific connector factory

use super::http_connector::{HttpConnector, HttpConnectorConfig, Provider};
use crate::connectors::DEFAULT_CONNECTOR_TIMEOUT_SECS;
use chrono::{DateTime, Utc};
use gate_core::Result;
use gate_core::router::types::{ConnectorCapabilities, CostStructure, Protocol};
use http::header::{AUTHORIZATION, USER_AGENT};
use http::{HeaderName, HeaderValue};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;
use std::time::Duration;
use url::Url;

pub(crate) const ANTHROPIC_VERSION: HeaderName = HeaderName::from_static("anthropic-version");
pub(crate) const ANTHROPIC_VERSION_VALUE: HeaderValue = HeaderValue::from_static("2023-06-01");

pub(crate) const ANTHROPIC_BETA: HeaderName = HeaderName::from_static("anthropic-beta");
pub(crate) const ANTHROPIC_BETA_OAUTH: HeaderValue = HeaderValue::from_static("oauth-2025-04-20");

pub(crate) const X_APP: HeaderName = HeaderName::from_static("x-app");
pub(crate) const X_APP_VALUE: HeaderValue = HeaderValue::from_static("cli");

pub(crate) const X_API_KEY: HeaderName = HeaderName::from_static("x-api-key");

pub(crate) const CLAUDE_CODE_USER_AGENT: HeaderValue =
    HeaderValue::from_static("claude-cli/1.0.102 (external, cli)");

/// Configuration for Anthropic provider
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    /// API key for Anthropic; if None, connector will operate in fallback mode and
    /// accept client-supplied keys via request headers.
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub timeout_seconds: Option<u64>,
    /// Optional connector ID to use in descriptions/registry keys
    pub sink_id: Option<String>,
}

/// Create an Anthropic connector
pub async fn create_sink(
    config: AnthropicConfig,
    allow_passthrough: bool,
) -> Result<HttpConnector> {
    let base_url = config
        .base_url
        .unwrap_or_else(|| "https://api.anthropic.com".to_string());

    // If we have an API key, fetch available models; otherwise leave dynamic.
    let models = if let Some(ref key) = config.api_key {
        fetch_models(&base_url, key).await?
    } else {
        Vec::new()
    };

    let timeout = Duration::from_secs(
        config
            .timeout_seconds
            .unwrap_or(DEFAULT_CONNECTOR_TIMEOUT_SECS),
    );
    let sink_config = HttpConnectorConfig {
        id: config
            .sink_id
            .clone()
            .unwrap_or_else(|| "provider://anthropic".to_string()),
        provider: Provider::Anthropic,
        base_url,
        api_key: config.api_key,
        models,
        timeout,
        max_retries: 3,
        accepted_protocols: vec![Protocol::Anthropic],
        capabilities: ConnectorCapabilities {
            supports_streaming: true,
            supports_batching: false,
            supports_tools: true,
            max_context_length: Some(200000),
            modalities: vec!["text".to_string(), "image".to_string()],
        },
        cost_structure: Some(CostStructure {
            input_cost_per_token: Decimal::from_str("0.000015").unwrap(), // $15 per 1M tokens
            output_cost_per_token: Decimal::from_str("0.000075").unwrap(), // $75 per 1M tokens
            cached_input_cost_per_token: Some(Decimal::from_str("0.0000075").unwrap()), // 50% discount
            currency: "USD".to_string(),
        }),
        allow_passthrough,
    };

    HttpConnector::new(sink_config)
}

/// Create a fallback Anthropic sink with no API key and default base URL.
/// This sink will accept client-supplied API keys via headers and capture
/// them after a successful response.
pub async fn create_fallback_sink(allow_passthrough: bool) -> Result<HttpConnector> {
    let config = AnthropicConfig {
        api_key: None,
        base_url: None,
        timeout_seconds: Some(DEFAULT_CONNECTOR_TIMEOUT_SECS),
        sink_id: Some("provider://anthropic/fallback".to_string()),
    };
    create_sink(config, allow_passthrough).await
}

#[derive(Deserialize)]
struct AnthropicModelsResponse {
    data: Vec<AnthropicModelItem>,
}

#[derive(Deserialize)]
struct AnthropicModelItem {
    id: String,
    _display_name: String,
    _created_at: DateTime<Utc>,
}

/// Fetch list of models from Anthropic
pub async fn fetch_models(base_url: &str, api_key: &str) -> Result<Vec<String>> {
    // Build URL safely: base_url + /v1/models
    let mut url = Url::parse(base_url)
        .map_err(|e| gate_core::Error::Internal(format!("Invalid base_url: {e}")))?;
    {
        let mut segs = url
            .path_segments_mut()
            .map_err(|_| gate_core::Error::Internal("Invalid base_url path".into()))?;
        segs.pop_if_empty();
        segs.extend(["v1", "models"]);
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| gate_core::Error::Internal(format!("Failed to build HTTP client: {e}")))?;

    let resp = client
        .get(url)
        .header(AUTHORIZATION, format!("Bearer {api_key}"))
        .header(ANTHROPIC_BETA, ANTHROPIC_BETA_OAUTH)
        .header(ANTHROPIC_VERSION, ANTHROPIC_VERSION_VALUE)
        .header(USER_AGENT, CLAUDE_CODE_USER_AGENT)
        .header(X_APP, X_APP_VALUE)
        .send()
        .await
        .map_err(|e| {
            gate_core::Error::ServiceUnavailable(format!("Anthropic models request failed: {e}"))
        })?
        .error_for_status()
        .map_err(|e| {
            gate_core::Error::ServiceUnavailable(format!("Anthropic models request error: {e}"))
        })?;

    let payload: AnthropicModelsResponse = resp.json().await.map_err(|e| {
        gate_core::Error::Internal(format!("Failed to parse Anthropic models: {e}"))
    })?;

    Ok(payload.data.into_iter().map(|m| m.id).collect())
}
