//! Generic HTTP-based sink for external LLM providers

use super::sse_parser::parse_sse;
use async_trait::async_trait;
use futures::StreamExt;
use gate_core::router::sink::{RequestContext, ResponseStream, Sink, SinkDescription};
use gate_core::router::types::{
    CostStructure, ModelList, Protocol, RequestStream, ResponseChunk, SinkCapabilities, SinkHealth,
    StopReason,
};
use gate_core::{Error, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error};

/// Provider type for HTTP sinks
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    OpenAI,
    Custom,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::Anthropic => write!(f, "anthropic"),
            Provider::OpenAI => write!(f, "openai"),
            Provider::Custom => write!(f, "custom"),
        }
    }
}

/// Configuration for an HTTP sink
#[derive(Debug, Clone)]
pub struct HttpSinkConfig {
    pub id: String,
    pub provider: Provider,
    pub base_url: String,
    pub api_key: Option<String>,
    pub models: Vec<String>,
    pub timeout: Duration,
    pub max_retries: u32,
    pub accepted_protocols: Vec<Protocol>,
    pub capabilities: SinkCapabilities,
    pub cost_structure: Option<CostStructure>,
}

/// HTTP-based sink for external LLM providers
pub struct HttpSink {
    config: HttpSinkConfig,
    client: Client,
    health: Arc<RwLock<SinkHealth>>,
}

impl HttpSink {
    /// Create a new HTTP sink
    pub fn new(config: HttpSinkConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| Error::Internal(format!("Failed to create HTTP client: {}", e)))?;

        let health = Arc::new(RwLock::new(SinkHealth {
            healthy: true,
            latency_ms: None,
            error_rate: 0.0,
            last_error: None,
            last_check: chrono::Utc::now(),
        }));

        Ok(Self {
            config,
            client,
            health,
        })
    }

    /// Get the appropriate authorization header for the provider
    fn auth_header(&self) -> Option<(&'static str, String)> {
        self.config
            .api_key
            .as_ref()
            .map(|key| match self.config.provider {
                Provider::Anthropic => ("x-api-key", key.clone()),
                Provider::OpenAI => ("Authorization", format!("Bearer {}", key)),
                Provider::Custom => ("Authorization", format!("Bearer {}", key)),
            })
    }

    /// Get provider-specific headers
    fn provider_headers(&self) -> Vec<(&'static str, &'static str)> {
        match self.config.provider {
            Provider::Anthropic => vec![("anthropic-version", "2023-06-01")],
            _ => vec![],
        }
    }

    /// Get the endpoint for the given protocol
    fn endpoint_for_protocol(&self, protocol: Protocol) -> Result<&'static str> {
        match (protocol, &self.config.provider) {
            (Protocol::OpenAIChat, Provider::OpenAI) => Ok("/v1/chat/completions"),
            (Protocol::OpenAIChat, Provider::Anthropic) => Ok("/v1/messages"), // Anthropic uses messages
            (Protocol::Anthropic, Provider::Anthropic) => Ok("/v1/messages"),
            (Protocol::OpenAIMessages, Provider::OpenAI) => Ok("/v1/messages"),
            _ => Err(Error::InvalidRoutingConfig(format!(
                "Provider {} doesn't support protocol {:?}",
                self.config.provider, protocol
            ))),
        }
    }

    /// Execute a streaming request
    async fn execute_streaming(
        &self,
        ctx: &RequestContext,
        mut request_stream: RequestStream,
    ) -> Result<ResponseStream> {
        // Get the first request chunk
        let request = request_stream
            .next()
            .await
            .ok_or_else(|| Error::InvalidRequest("Empty request stream".to_string()))??;

        let protocol = request_stream.protocol();
        let endpoint = self.endpoint_for_protocol(protocol)?;
        let url = format!("{}{}", self.config.base_url, endpoint);

        debug!("Executing streaming request to {}", url);

        // Build the HTTP request
        let mut req = self.client.post(&url).json(&request);

        // Add authentication
        if let Some((header_name, header_value)) = self.auth_header() {
            req = req.header(header_name, header_value);
        }

        // Add provider headers
        for (name, value) in self.provider_headers() {
            req = req.header(name, value);
        }

        // Add trace headers from context
        for (name, value) in &ctx.headers {
            if !matches!(
                name.as_str(),
                "host" | "content-length" | "content-type" | "authorization" | "x-api-key"
            ) {
                req = req.header(name, value);
            }
        }

        // Send the request
        let response = req.send().await.map_err(|e| {
            Error::ServiceUnavailable(format!(
                "Failed to send request to {}: {}",
                self.config.provider, e
            ))
        })?;

        let status = response.status();
        if !status.is_success() {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error".to_string());
            return Err(Error::ServiceUnavailable(format!(
                "{} returned error {}: {}",
                self.config.provider, status, error_body
            )));
        }

        // Check if response is SSE stream
        let is_sse = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("text/event-stream"))
            .unwrap_or(false);

        if is_sse {
            // Parse SSE stream into ResponseChunks
            self.parse_sse_stream(response, protocol).await
        } else {
            // Non-streaming response - convert to single chunk
            let body = response
                .json::<JsonValue>()
                .await
                .map_err(|e| Error::Internal(format!("Failed to parse response: {}", e)))?;

            let chunks = vec![
                Ok(ResponseChunk::Content(body)),
                Ok(ResponseChunk::Stop {
                    reason: StopReason::Complete,
                    error: None,
                    cost: None,
                }),
            ];

            Ok(Box::pin(futures::stream::iter(chunks)))
        }
    }

    /// Parse SSE stream into ResponseChunks
    async fn parse_sse_stream(
        &self,
        response: reqwest::Response,
        _protocol: Protocol,
    ) -> Result<ResponseStream> {
        let stream = response.bytes_stream();
        let sse_stream = parse_sse(stream);

        let provider = self.config.provider.clone();
        let stream = sse_stream.map(move |result| {
            match result {
                Ok(event) => {
                    // Handle special [DONE] message
                    if event.data == "[DONE]" {
                        return Ok(ResponseChunk::Stop {
                            reason: StopReason::Complete,
                            error: None,
                            cost: None,
                        });
                    }

                    // Try to parse as JSON
                    if let Ok(json) = serde_json::from_str::<JsonValue>(&event.data) {
                        // Check if this is an error response
                        if let Some(error) = json.get("error") {
                            return Ok(ResponseChunk::Stop {
                                reason: StopReason::Error,
                                error: Some(error.to_string()),
                                cost: None,
                            });
                        }

                        return Ok(ResponseChunk::Content(json));
                    }

                    // If not JSON, return as string
                    Ok(ResponseChunk::Content(JsonValue::String(event.data)))
                }
                Err(e) => {
                    error!("SSE parse error from {}: {}", provider, e);
                    Ok(ResponseChunk::Stop {
                        reason: StopReason::Error,
                        error: Some(format!("Stream error: {}", e)),
                        cost: None,
                    })
                }
            }
        });

        Ok(Box::pin(stream))
    }

    /// Execute a non-streaming request
    async fn execute_non_streaming(
        &self,
        ctx: &RequestContext,
        request_stream: RequestStream,
    ) -> Result<ResponseStream> {
        // For non-streaming, we still execute the request but accumulate the response
        self.execute_streaming(ctx, request_stream).await
    }
}

#[async_trait]
impl Sink for HttpSink {
    async fn describe(&self) -> SinkDescription {
        SinkDescription {
            id: self.config.id.clone(),
            accepted_protocols: self.config.accepted_protocols.clone(),
            models: if self.config.models.is_empty() {
                ModelList::Dynamic
            } else {
                ModelList::Static(self.config.models.clone())
            },
            capabilities: self.config.capabilities.clone(),
            cost_structure: self.config.cost_structure.clone(),
        }
    }

    async fn probe(&self) -> SinkHealth {
        // In a real implementation, we might ping the provider's API
        self.health.read().await.clone()
    }

    async fn execute(
        &self,
        ctx: &RequestContext,
        mut request_stream: RequestStream,
    ) -> Result<ResponseStream> {
        // Check if streaming is requested
        let first_chunk = request_stream.next().await;
        if let Some(Ok(ref json)) = first_chunk {
            let is_streaming = json
                .get("stream")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            // Reconstruct the stream with the first chunk
            let chunks = vec![first_chunk.unwrap()];
            let remaining = request_stream;
            let protocol = remaining.protocol();
            let reconstructed = RequestStream::new(
                protocol,
                Box::pin(futures::stream::iter(chunks).chain(remaining)),
            );

            if is_streaming {
                self.execute_streaming(ctx, reconstructed).await
            } else {
                self.execute_non_streaming(ctx, reconstructed).await
            }
        } else {
            Err(Error::InvalidRequest("Empty request stream".to_string()))
        }
    }
}
