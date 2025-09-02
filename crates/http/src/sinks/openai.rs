//! OpenAI-specific sink factory

use super::http_sink::{HttpSink, HttpSinkConfig, Provider};
use gate_core::Result;
use gate_core::router::types::{CostStructure, Protocol, SinkCapabilities};
use rust_decimal::Decimal;
use std::str::FromStr;
use std::time::Duration;

/// Configuration for OpenAI provider
#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    pub api_key: String,
    pub base_url: Option<String>,
    pub models: Option<Vec<String>>,
    pub timeout_seconds: Option<u64>,
}

/// Create an OpenAI sink
pub fn create_sink(config: OpenAIConfig) -> Result<HttpSink> {
    let base_url = config
        .base_url
        .unwrap_or_else(|| "https://api.openai.com".to_string());

    // Default OpenAI models if not specified
    let models = config.models.unwrap_or_else(|| {
        vec![
            // GPT-4 models
            "gpt-4-turbo-preview".to_string(),
            "gpt-4-turbo".to_string(),
            "gpt-4-turbo-2024-04-09".to_string(),
            "gpt-4-0125-preview".to_string(),
            "gpt-4-1106-preview".to_string(),
            "gpt-4-vision-preview".to_string(),
            "gpt-4".to_string(),
            "gpt-4-0613".to_string(),
            "gpt-4-32k".to_string(),
            "gpt-4-32k-0613".to_string(),
            // GPT-4o models
            "gpt-4o".to_string(),
            "gpt-4o-2024-05-13".to_string(),
            "gpt-4o-2024-08-06".to_string(),
            "gpt-4o-mini".to_string(),
            "gpt-4o-mini-2024-07-18".to_string(),
            // GPT-3.5 models
            "gpt-3.5-turbo".to_string(),
            "gpt-3.5-turbo-0125".to_string(),
            "gpt-3.5-turbo-1106".to_string(),
            "gpt-3.5-turbo-16k".to_string(),
            // O1 models
            "o1-preview".to_string(),
            "o1-preview-2024-09-12".to_string(),
            "o1-mini".to_string(),
            "o1-mini-2024-09-12".to_string(),
        ]
    });

    let timeout = Duration::from_secs(config.timeout_seconds.unwrap_or(30));

    let sink_config = HttpSinkConfig {
        id: "provider://openai".to_string(),
        provider: Provider::OpenAI,
        base_url,
        api_key: Some(config.api_key),
        models,
        timeout,
        max_retries: 3,
        accepted_protocols: vec![
            Protocol::OpenAIChat,
            Protocol::OpenAIMessages,
            Protocol::OpenAICompletions,
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

/// Create an OpenAI sink from environment variables
pub fn create_sink_from_env() -> Result<HttpSink> {
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| gate_core::Error::InvalidConfig("OPENAI_API_KEY not set".to_string()))?;

    let base_url = std::env::var("OPENAI_BASE_URL").ok();

    let models = std::env::var("OPENAI_MODELS")
        .ok()
        .map(|s| s.split(',').map(|m| m.trim().to_string()).collect());

    let timeout_seconds = std::env::var("OPENAI_TIMEOUT")
        .ok()
        .and_then(|s| s.parse().ok());

    create_sink(OpenAIConfig {
        api_key,
        base_url,
        models,
        timeout_seconds,
    })
}
