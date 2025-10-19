//! OpenAI-specific connector factory

use crate::connectors::DEFAULT_CONNECTOR_TIMEOUT_SECS;

use super::http_connector::{HttpConnector, HttpConnectorConfig, Provider};
use gate_core::Result;
use gate_core::router::types::{ConnectorCapabilities, CostStructure, Protocol};
use rust_decimal::Decimal;
use std::str::FromStr;
use std::time::Duration;

/// Configuration for OpenAI provider
#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub models: Option<Vec<String>>,
    pub timeout_seconds: Option<u64>,
    /// Optional connector ID to use in descriptions/registry keys
    pub sink_id: Option<String>,
}

/// Create an OpenAI connector
pub fn create_sink(config: OpenAIConfig, allow_passthrough: bool) -> Result<HttpConnector> {
    let base_url = config
        .base_url
        .unwrap_or_else(|| "https://api.openai.com".to_string());

    // No hardcoded model list; rely on configured list or dynamic discovery.
    // Empty list means dynamic support (accept unknown models for routing intent).
    let models = config.models.unwrap_or_default();

    let timeout = Duration::from_secs(
        config
            .timeout_seconds
            .unwrap_or(DEFAULT_CONNECTOR_TIMEOUT_SECS),
    );

    let sink_config = HttpConnectorConfig {
        id: config
            .sink_id
            .clone()
            .unwrap_or_else(|| "provider://openai".to_string()),
        provider: Provider::OpenAI,
        base_url,
        api_key: config.api_key,
        models,
        timeout,
        max_retries: 3,
        accepted_protocols: vec![
            Protocol::OpenAIChat,
            Protocol::OpenAIMessages,
            Protocol::OpenAICompletions,
            Protocol::OpenAIResponses,
        ],
        capabilities: ConnectorCapabilities {
            supports_streaming: true,
            supports_batching: false,
            supports_tools: true,
            max_context_length: Some(128000), // GPT-4 Turbo supports up to 128k
            modalities: vec!["text".to_string(), "image".to_string()], // GPT-4V supports vision
        },
        cost_structure: Some(CostStructure {
            // Pricing for GPT-4 Turbo
            input_cost_per_token: Decimal::from_str("0.00001").unwrap(), // $10 per 1M tokens
            output_cost_per_token: Decimal::from_str("0.00003").unwrap(), // $30 per 1M tokens
            cached_input_cost_per_token: None, // OpenAI doesn't have cached pricing
            currency: "USD".to_string(),
        }),
        allow_passthrough,
    };

    HttpConnector::new(sink_config)
}

/// Create a fallback OpenAI connector with no API key and default base URL.
/// This connector will accept client-supplied API keys via Authorization headers.
pub fn create_fallback_sink(allow_passthrough: bool) -> Result<HttpConnector> {
    let config = OpenAIConfig {
        api_key: None,
        base_url: None,
        models: None,
        timeout_seconds: Some(DEFAULT_CONNECTOR_TIMEOUT_SECS),
        sink_id: Some("provider://openai/fallback".to_string()),
    };
    create_sink(config, allow_passthrough)
}

/// Create a fallback Codex connector (ChatGPT Codex backend) with no API key and default base URL.
/// Accepts OpenAI Responses protocol with dynamic models; expects OAuth Bearer tokens.
pub fn create_codex_fallback_sink(allow_passthrough: bool) -> Result<HttpConnector> {
    // Ensure trailing slash so URL::join appends endpoint under codex/
    let base_url = "https://chatgpt.com/backend-api/codex/".to_string();
    let timeout = std::time::Duration::from_secs(DEFAULT_CONNECTOR_TIMEOUT_SECS);

    let sink_config = super::http_connector::HttpConnectorConfig {
        id: "provider://openai/codex".to_string(),
        provider: super::http_connector::Provider::OpenAICodex,
        base_url,
        api_key: None,
        models: Vec::new(),
        timeout,
        max_retries: 3,
        accepted_protocols: vec![Protocol::OpenAIResponses],
        capabilities: ConnectorCapabilities {
            supports_streaming: true,
            supports_batching: false,
            supports_tools: true,
            max_context_length: None,
            modalities: vec!["text".to_string()],
        },
        cost_structure: None,
        allow_passthrough,
    };

    super::http_connector::HttpConnector::new(sink_config)
}
