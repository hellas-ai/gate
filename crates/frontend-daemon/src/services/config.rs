//! Configuration API service

use gate_frontend_common::create_authenticated_client;
use reqwest::Method;
use serde_json::Value;

/// Configuration API service
#[derive(Clone)]
pub struct ConfigApiService;

impl ConfigApiService {
    /// Create a new config API service
    pub fn new() -> Self {
        Self
    }
}

impl Default for ConfigApiService {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigApiService {
    /// Get the full configuration (requires admin authentication)
    pub async fn get_config(&self) -> Result<Value, String> {
        let client = create_authenticated_client()
            .map_err(|e| format!("Failed to get client: {e}"))?
            .ok_or_else(|| "Not authenticated".to_string())?;

        let response = client
            .request(Method::GET, "/api/config")
            .send()
            .await
            .map_err(|e| format!("Failed to get config: {e}"))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("Failed to get config: {error_text}"));
        }

        #[derive(serde::Deserialize)]
        struct ConfigResponse {
            config: Value,
        }

        let config_response: ConfigResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse config: {e}"))?;

        Ok(config_response.config)
    }

    /// Update the configuration (requires admin authentication)
    pub async fn update_config(&self, config: Value) -> Result<Value, String> {
        let client = create_authenticated_client()
            .map_err(|e| format!("Failed to get client: {e}"))?
            .ok_or_else(|| "Not authenticated".to_string())?;

        #[derive(serde::Serialize)]
        struct UpdateRequest {
            config: Value,
        }

        let response = client
            .request(Method::PUT, "/api/config")
            .json(&UpdateRequest {
                config: config.clone(),
            })
            .send()
            .await
            .map_err(|e| format!("Failed to update config: {e}"))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("Failed to update config: {error_text}"));
        }

        #[derive(serde::Deserialize)]
        struct ConfigResponse {
            config: Value,
        }

        let config_response: ConfigResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse config response: {e}"))?;

        Ok(config_response.config)
    }
}
