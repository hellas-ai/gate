//! Configuration management client methods

use crate::client::{ClientError, GateClient};
use crate::types::{ConfigPatchRequest, ConfigResponse, ConfigUpdateRequest};
use reqwest::Method;
use serde_json::Value;

impl GateClient {
    /// Get the full configuration
    pub async fn get_config(&self) -> Result<ConfigResponse, ClientError> {
        let request = self.request(Method::GET, "/api/config")?;
        self.execute(request).await
    }

    /// Get configuration at a specific path
    pub async fn get_config_path(&self, path: &str) -> Result<Value, ClientError> {
        let request = self.request(Method::GET, &format!("/api/config/{path}"))?;
        self.execute(request).await
    }

    /// Update the full configuration
    pub async fn update_config(&self, config: Value) -> Result<ConfigResponse, ClientError> {
        let request = self
            .request(Method::PUT, "/api/config")?
            .json(&ConfigUpdateRequest { config });
        self.execute(request).await
    }

    /// Update configuration at a specific path
    pub async fn patch_config_path(&self, path: &str, value: Value) -> Result<Value, ClientError> {
        let request = self
            .request(Method::PATCH, &format!("/api/config/{path}"))?
            .json(&ConfigPatchRequest { value });
        self.execute(request).await
    }
}
