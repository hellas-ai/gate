//! Model detection for upstream providers

use crate::forwarding::ForwardingConfig;
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

/// OpenAI models response format
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModelsResponse {
    data: Vec<Model>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Model {
    id: String,
    object: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    created: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owned_by: Option<String>,
}

/// Detect available models from an upstream provider
pub async fn detect_models(config: &ForwardingConfig) -> Vec<String> {
    #[cfg(not(target_arch = "wasm32"))]
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_seconds))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            warn!("Failed to create HTTP client for model detection: {}", e);
            return vec![];
        }
    };

    #[cfg(target_arch = "wasm32")]
    let client = match reqwest::Client::builder().build() {
        Ok(client) => client,
        Err(e) => {
            warn!("Failed to create HTTP client for model detection: {}", e);
            return vec![];
        }
    };

    let url = format!("{}/models", config.base_url);
    let mut req = client.get(&url);

    // Add authentication header
    if let Some((header_name, header_value)) = config.auth_header() {
        req = req.header(header_name, header_value);
    }

    // Add provider-specific headers
    for (name, value) in config.provider_headers() {
        req = req.header(name, value);
    }

    match req.send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<ModelsResponse>().await {
                    Ok(models_response) => {
                        let models: Vec<String> =
                            models_response.data.into_iter().map(|m| m.id).collect();
                        info!("Detected {} models from upstream", models.len());
                        models
                    }
                    Err(e) => {
                        warn!("Failed to parse models response: {}", e);
                        vec![]
                    }
                }
            } else {
                warn!("Models endpoint returned status: {}", response.status());
                vec![]
            }
        }
        Err(e) => {
            warn!("Failed to query models endpoint: {}", e);
            vec![]
        }
    }
}
