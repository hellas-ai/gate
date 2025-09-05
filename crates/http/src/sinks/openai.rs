//! OpenAI-specific sink factory

use crate::sinks::DEFAULT_SINK_TIMEOUT_SECS;

use super::http_sink::{HttpSink, HttpSinkConfig, Provider};
use gate_core::Result;
use gate_core::router::types::{CostStructure, Protocol, SinkCapabilities};
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
    /// Optional sink ID to use in descriptions/registry keys
    pub sink_id: Option<String>,
}

/// Create an OpenAI sink
pub fn create_sink(config: OpenAIConfig) -> Result<HttpSink> {
    let base_url = config
        .base_url
        .unwrap_or_else(|| "https://api.openai.com".to_string());

    // No hardcoded model list; rely on configured list or dynamic discovery.
    // Empty list means dynamic support (accept unknown models for routing intent).
    let models = config.models.unwrap_or_default();

    let timeout = Duration::from_secs(config.timeout_seconds.unwrap_or(DEFAULT_SINK_TIMEOUT_SECS));

    let sink_config = HttpSinkConfig {
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
        capabilities: SinkCapabilities {
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
    };

    HttpSink::new(sink_config)
}

/// Create a fallback OpenAI sink with no API key and default base URL.
/// This sink will accept client-supplied API keys via Authorization headers.
pub fn create_fallback_sink() -> Result<HttpSink> {
    let config = OpenAIConfig {
        api_key: None,
        base_url: None,
        models: None,
        timeout_seconds: Some(DEFAULT_SINK_TIMEOUT_SECS),
        sink_id: Some("provider://openai/fallback".to_string()),
    };
    create_sink(config)
}

/// Create a fallback Codex sink (ChatGPT Codex backend) with no API key and default base URL.
/// Accepts OpenAI Responses protocol with dynamic models; expects OAuth Bearer tokens.
pub fn create_codex_fallback_sink() -> Result<HttpSink> {
    // Ensure trailing slash so URL::join appends endpoint under codex/
    let base_url = "https://chatgpt.com/backend-api/codex/".to_string();
    let timeout = std::time::Duration::from_secs(DEFAULT_SINK_TIMEOUT_SECS);

    let sink_config = super::http_sink::HttpSinkConfig {
        id: "provider://openai/codex".to_string(),
        provider: super::http_sink::Provider::OpenAICodex,
        base_url,
        api_key: None,
        models: Vec::new(),
        timeout,
        max_retries: 3,
        accepted_protocols: vec![Protocol::OpenAIResponses],
        capabilities: SinkCapabilities {
            supports_streaming: true,
            supports_batching: false,
            supports_tools: true,
            max_context_length: None,
            modalities: vec!["text".to_string()],
        },
        cost_structure: None,
    };

    super::http_sink::HttpSink::new(sink_config)
}
