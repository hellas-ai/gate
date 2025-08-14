//! Configuration API service

use gate_frontend_common::{client::ClientError, create_authenticated_client};
use reqwest::Method;
use serde::{Deserialize, Serialize};
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
    pub async fn get_config(&self) -> Result<Value, ClientError> {
        let client = create_authenticated_client()?
            .ok_or_else(|| ClientError::Configuration("Not authenticated".into()))?;

        #[derive(Deserialize)]
        struct ConfigResponse {
            config: Value,
        }

        let response: ConfigResponse = client
            .execute(client.request(Method::GET, "/api/config"))
            .await?;

        Ok(response.config)
    }

    /// Update the configuration (requires admin authentication)
    pub async fn update_config(&self, config: Value) -> Result<Value, ClientError> {
        let client = create_authenticated_client()?
            .ok_or_else(|| ClientError::Configuration("Not authenticated".into()))?;

        #[derive(Serialize)]
        struct UpdateRequest {
            config: Value,
        }

        #[derive(Deserialize)]
        struct ConfigResponse {
            config: Value,
        }

        let response: ConfigResponse = client
            .execute(
                client
                    .request(Method::PUT, "/api/config")
                    .json(&UpdateRequest { config }),
            )
            .await?;

        Ok(response.config)
    }
}
