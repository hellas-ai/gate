//! Bootstrap service for initial admin setup

use crate::client::create_public_client;
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
pub struct BootstrapService;

impl BootstrapService {
    /// Create a new bootstrap service
    pub fn new() -> Self {
        Self
    }

    /// Check if bootstrap is needed
    pub async fn check_status(&self) -> Result<BootstrapStatus, String> {
        let client = create_public_client().map_err(|e| format!("Failed to get client: {e}"))?;

        let response = client
            .request(reqwest::Method::GET, "/auth/bootstrap/status")
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

impl Default for BootstrapService {
    fn default() -> Self {
        Self::new()
    }
}
