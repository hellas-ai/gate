//! Configuration API service

use gate_frontend_common::ClientError;
use gate_http::client::AuthenticatedGateClient;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Configuration API service
#[derive(Clone)]
pub struct ConfigApiService {
    client: AuthenticatedGateClient,
}

impl ConfigApiService {
    /// Create a new config API service
    pub fn new(client: AuthenticatedGateClient) -> Self {
        Self { client }
    }
}

impl ConfigApiService {
    /// Get the full configuration (requires admin authentication)
    pub async fn get_config(&self) -> Result<Value, ClientError> {
        #[derive(Deserialize)]
        struct ConfigResponse {
            config: Value,
        }

        let response: ConfigResponse = self
            .client
            .execute(self.client.request(Method::GET, "/api/config")?)
            .await?;

        Ok(response.config)
    }

    /// Update the configuration (requires admin authentication)
    pub async fn update_config(&self, config: Value) -> Result<Value, ClientError> {
        #[derive(Serialize)]
        struct UpdateRequest {
            config: Value,
        }

        #[derive(Deserialize)]
        struct ConfigResponse {
            config: Value,
        }

        let response: ConfigResponse = self
            .client
            .execute(
                self.client
                    .request(Method::PUT, "/api/config")?
                    .json(&UpdateRequest { config }),
            )
            .await?;

        Ok(response.config)
    }
}
