//! Bootstrap service for initial admin setup

use gate_http::client::PublicGateClient;
use serde::{Deserialize, Serialize};

/// Bootstrap status response
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BootstrapStatus {
    pub needs_bootstrap: bool,
    pub is_complete: bool,
    pub message: String,
}

/// Bootstrap API service
#[derive(Clone)]
pub struct BootstrapService {
    client: PublicGateClient,
}

impl BootstrapService {
    /// Create a new bootstrap service
    pub fn new(client: PublicGateClient) -> Self {
        Self { client }
    }

    /// Check if bootstrap is needed
    pub async fn check_status(&self) -> Result<BootstrapStatus, String> {
        let request = self
            .client
            .request(reqwest::Method::GET, "/auth/bootstrap/status")
            .map_err(|e| format!("Failed to check bootstrap status: {e}"))?;
        let response = request
            .send()
            .await
            .map_err(|e| format!("Failed to check bootstrap status: {e}"))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(format!("Failed to check bootstrap status: {error_text}"));
        }

        response
            .json()
            .await
            .map_err(|e| format!("Failed to parse bootstrap status: {e}"))
    }
}
