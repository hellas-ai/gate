//! Transport abstraction for executing outbound HTTP-like requests

use crate::Result;
use futures::Stream;
use http::HeaderMap;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::pin::Pin;
use std::time::Duration;

/// Stream of raw response body bytes
pub type BodyStream = Pin<Box<dyn Stream<Item = Result<Vec<u8>>> + Send>>;

/// A JSON POST request to an upstream endpoint
#[derive(Debug, Clone)]
pub struct TransportRequest {
    pub url: String,
    pub headers: HeaderMap,
    pub body: JsonValue,
    pub timeout: Option<Duration>,
}

/// Response from transport for a JSON POST
pub struct TransportResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: BodyStream,
}

/// A generic transport for posting JSON and streaming responses
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    async fn post_json(&self, req: TransportRequest) -> Result<TransportResponse>;
}
