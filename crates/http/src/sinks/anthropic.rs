//! Anthropic-specific sink factory

use super::http_sink::{HttpSink, HttpSinkConfig, Provider};
use gate_core::Result;
use gate_core::router::types::{CostStructure, Protocol, SinkCapabilities};
use rust_decimal::Decimal;
use std::str::FromStr;
use std::time::Duration;

/// Configuration for Anthropic provider
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    pub api_key: String,
    pub base_url: Option<String>,
    pub models: Option<Vec<String>>,
    pub timeout_seconds: Option<u64>,
}

/// Create an Anthropic sink
pub fn create_sink(config: AnthropicConfig) -> Result<HttpSink> {
    let base_url = config
        .base_url
        .unwrap_or_else(|| "https://api.anthropic.com".to_string());

    // Default Anthropic models if not specified
    let models = config.models.unwrap_or_else(|| {
        vec![
            "claude-3-opus-20240229".to_string(),
            "claude-3-sonnet-20240229".to_string(),
            "claude-3-haiku-20240307".to_string(),
            "claude-3-5-sonnet-20241022".to_string(),
            "claude-3-5-haiku-20241022".to_string(),
            "claude-2.1".to_string(),
            "claude-2.0".to_string(),
            "claude-instant-1.2".to_string(),
        ]
    });

    let timeout = Duration::from_secs(config.timeout_seconds.unwrap_or(30));

    let sink_config = HttpSinkConfig {
        id: "provider://anthropic".to_string(),
        provider: Provider::Anthropic,
        base_url,
        api_key: Some(config.api_key),
        models,
        timeout,
        max_retries: 3,
        accepted_protocols: vec![Protocol::Anthropic, Protocol::OpenAIChat],
        capabilities: SinkCapabilities {
            supports_streaming: true,
            supports_batching: false,
            supports_tools: true,
            max_context_length: Some(200000), // Claude 3 supports up to 200k
            modalities: vec!["text".to_string(), "image".to_string()], // Claude 3 supports vision
        },
        cost_structure: Some(CostStructure {
            // Pricing for Claude 3 Opus (most expensive)
            input_cost_per_token: Decimal::from_str("0.000015").unwrap(), // $15 per 1M tokens
            output_cost_per_token: Decimal::from_str("0.000075").unwrap(), // $75 per 1M tokens
            cached_input_cost_per_token: Some(Decimal::from_str("0.0000075").unwrap()), // 50% discount
            currency: "USD".to_string(),
        }),
    };

    HttpSink::new(sink_config)
}

/// Create an Anthropic sink from environment variables
pub fn create_sink_from_env() -> Result<HttpSink> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| gate_core::Error::InvalidConfig("ANTHROPIC_API_KEY not set".to_string()))?;

    let base_url = std::env::var("ANTHROPIC_BASE_URL").ok();

    let models = std::env::var("ANTHROPIC_MODELS")
        .ok()
        .map(|s| s.split(',').map(|m| m.trim().to_string()).collect());

    let timeout_seconds = std::env::var("ANTHROPIC_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok());

    create_sink(AnthropicConfig {
        api_key,
        base_url,
        models,
        timeout_seconds,
    })
}
