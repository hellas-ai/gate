//! Protocol conversion and capabilities

mod capabilities;
mod convert;

pub use crate::router::types::RequestCapabilities;
pub use capabilities::extract_capabilities;
pub use convert::{can_convert, conversion_loss, convert_request, convert_response};

use super::types::Protocol;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Protocol conversion information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolConversion {
    pub from: Protocol,
    pub to: Protocol,
    pub expected_loss: Vec<String>,
}

/// Extract model from request
pub fn extract_model(request: &JsonValue) -> Option<String> {
    request
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}
