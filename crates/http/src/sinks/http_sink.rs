//! Generic HTTP-based sink for external LLM providers

use crate::sinks::anthropic::{
    ANTHROPIC_BETA, ANTHROPIC_BETA_OAUTH, ANTHROPIC_VERSION, ANTHROPIC_VERSION_VALUE,
    CLAUDE_CODE_USER_AGENT, X_API_KEY, X_APP, X_APP_VALUE,
};

use super::sse_parser::parse_sse;
use async_trait::async_trait;
use futures::StreamExt;
use gate_core::router::sink::{RequestContext, ResponseStream, Sink, SinkDescription};
use gate_core::router::types::{
    CostStructure, ModelList, Protocol, RequestStream, ResponseChunk, SinkCapabilities, SinkHealth,
    StopReason,
};
use gate_core::{Error, Result};
use http::header::{AUTHORIZATION, USER_AGENT};
use http::{HeaderName, HeaderValue};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use url::{Url, form_urlencoded};

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
            .map_err(|e| Error::Internal(format!("Failed to create HTTP client: {e}")))?;

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
    fn auth_header(&self) -> Option<(HeaderName, HeaderValue)> {
        self.config
            .api_key
            .as_ref()
            .and_then(|key| match self.config.provider {
                Provider::Anthropic => {
                    if key.starts_with("sk-ant-oat01-") {
                        HeaderValue::from_str(&format!("Bearer {key}"))
                            .ok()
                            .map(|v| (AUTHORIZATION, v))
                    } else {
                        HeaderValue::from_str(key).ok().map(|v| (X_API_KEY, v))
                    }
                }
                Provider::OpenAI | Provider::Custom => {
                    HeaderValue::from_str(&format!("Bearer {key}"))
                        .ok()
                        .map(|v| (AUTHORIZATION, v))
                }
            })
    }

    /// Determine auth header from a client-supplied key (Anthropic only for now)
    fn inferred_auth_from_client_headers(
        &self,
        ctx: &RequestContext,
    ) -> Option<(HeaderName, HeaderValue)> {
        if let Provider::Anthropic = self.config.provider {
            // Prefer x-api-key when provided
            if let Some(val) = ctx.headers.get("x-api-key")
                && let Ok(key) = val.to_str()
                && !key.is_empty()
                && let Ok(hv) = HeaderValue::from_str(key)
            {
                return Some((X_API_KEY, hv));
            }
            // Fallback to Authorization: Bearer ...
            if let Some(val) = ctx.headers.get("authorization")
                && let Ok(auth) = val.to_str()
                && auth.starts_with("Bearer sk-ant-")
                && let Ok(hv) = HeaderValue::from_str(auth)
            {
                return Some((AUTHORIZATION, hv));
            }
        }
        None
    }

    /// Get provider-specific headers
    fn provider_headers(&self) -> Vec<(HeaderName, HeaderValue)> {
        match self.config.provider {
            Provider::Anthropic => vec![
                (ANTHROPIC_BETA, ANTHROPIC_BETA_OAUTH),
                (ANTHROPIC_VERSION, ANTHROPIC_VERSION_VALUE),
                (USER_AGENT, CLAUDE_CODE_USER_AGENT),
                (X_APP, X_APP_VALUE),
            ],
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

    /// Build the upstream URL from base_url, endpoint, and forwarded query
    fn build_url(&self, ctx: &RequestContext, protocol: Protocol) -> Result<Url> {
        let endpoint = self.endpoint_for_protocol(protocol)?;
        let mut url = Url::parse(&self.config.base_url).map_err(|e| {
            Error::Internal(format!(
                "Invalid base_url for {}: {}",
                self.config.provider, e
            ))
        })?;
        url = url
            .join(endpoint.trim_start_matches('/'))
            .map_err(|e| Error::Internal(format!("Failed to join endpoint: {e}")))?;
        if let Some(q) = &ctx.query {
            let mut qp = url.query_pairs_mut();
            qp.clear()
                .extend_pairs(form_urlencoded::parse(q.as_bytes()));
        }
        Ok(url)
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
        let url = self.build_url(ctx, protocol)?;

        // Build the HTTP request
        let mut req = self.client.post(url).json(&request);

        // Add authentication
        if let Some((header_name, header_value)) = self.auth_header() {
            // Use configured API key
            req = req.header(header_name, header_value);
        } else {
            // Use client-supplied key when no configured key exists
            if let Some((hn, hv)) = self.inferred_auth_from_client_headers(ctx) {
                req = req.header(hn, hv);
            }
        }

        // Add provider headers
        for (name, value) in self.provider_headers() {
            req = req.header(name, value);
        }

        // Add trace headers from context (typed)
        for (name, value) in ctx.headers.iter() {
            let n = name.as_str();
            if matches!(
                n,
                "host" | "content-length" | "content-type" | "authorization" | "x-api-key"
            ) {
                continue;
            }
            req = req.header(name, value);
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
            // Capture upstream headers to forward
            let mut hdrs = std::collections::HashMap::new();
            for (name, value) in response.headers().iter() {
                if let Ok(v) = value.to_str() {
                    hdrs.insert(name.to_string(), v.to_string());
                }
            }

            // Parse SSE stream into ResponseChunks
            let sse_stream = self.parse_sse_stream(response, protocol).await?;

            // Prepend headers chunk
            let stream = futures::stream::once(async move { Ok(ResponseChunk::Headers(hdrs)) })
                .chain(sse_stream);
            Ok(Box::pin(stream))
        } else {
            // Capture upstream headers to forward
            let mut hdrs = std::collections::HashMap::new();
            for (name, value) in response.headers().iter() {
                if let Ok(v) = value.to_str() {
                    hdrs.insert(name.to_string(), v.to_string());
                }
            }

            // Non-streaming response - convert to single chunk
            let text = response
                .text()
                .await
                .map_err(|e| Error::Internal(format!("Failed to read response: {e}")))?;
            debug!("Non-streaming response: {}", text);
            let body: JsonValue = serde_json::from_str(&text).map_err(|e| {
                Error::Internal(format!(
                    "Failed to parse JSON response from {}: {e}",
                    self.config.provider
                ))
            })?;
            let chunks = vec![
                Ok(ResponseChunk::Headers(hdrs)),
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
                        error: Some(format!("Stream error: {e}")),
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

#[cfg(test)]
mod tests {
    use super::*;
    use gate_core::router::sink::RouterIdentityContext;
    use gate_core::tracing::CorrelationId;

    fn make_ctx(headers: Vec<(&str, &str)>) -> RequestContext {
        let mut hmap = http::HeaderMap::new();
        for (k, v) in headers {
            let name = http::HeaderName::from_lowercase(k.as_bytes()).unwrap();
            let val = http::HeaderValue::from_str(v).unwrap();
            hmap.insert(name, val);
        }
        RequestContext {
            identity: gate_core::access::SubjectIdentity::new(
                "test",
                "test",
                RouterIdentityContext::default(),
            ),
            correlation_id: CorrelationId::new(),
            headers: hmap,
            query: None,
            trace_id: None,
            metadata: Default::default(),
        }
    }

    #[tokio::test]
    async fn test_inferred_auth_from_client_headers_anthropic() {
        let sink = HttpSink::new(HttpSinkConfig {
            id: "provider://anthropic".into(),
            provider: Provider::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            api_key: None,
            models: vec![],
            timeout: std::time::Duration::from_secs(5),
            max_retries: 0,
            accepted_protocols: vec![Protocol::Anthropic],
            capabilities: SinkCapabilities {
                supports_streaming: true,
                supports_batching: false,
                supports_tools: true,
                max_context_length: None,
                modalities: vec!["text".to_string()],
            },
            cost_structure: None,
        })
        .expect("create sink");

        // x-api-key preferred
        let ctx = make_ctx(vec![("x-api-key", "sk-ant-abc123")]);
        let res = sink.inferred_auth_from_client_headers(&ctx);
        assert!(res.is_some());
        let (hn, hv) = res.unwrap();
        assert_eq!(hn, X_API_KEY);
        assert_eq!(hv, http::HeaderValue::from_static("sk-ant-abc123"));

        // Authorization Bearer fallback
        let ctx = make_ctx(vec![("authorization", "Bearer sk-ant-oat01-xyz")]);
        let res = sink.inferred_auth_from_client_headers(&ctx);
        assert!(res.is_some());
        let (hn, hv) = res.unwrap();
        assert_eq!(hn, AUTHORIZATION);
        assert_eq!(
            hv,
            http::HeaderValue::from_static("Bearer sk-ant-oat01-xyz")
        );
    }

    #[tokio::test]
    async fn test_build_url_forwards_query() {
        let sink = HttpSink::new(HttpSinkConfig {
            id: "provider://anthropic".into(),
            provider: Provider::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            api_key: None,
            models: vec![],
            timeout: std::time::Duration::from_secs(5),
            max_retries: 0,
            accepted_protocols: vec![Protocol::Anthropic],
            capabilities: SinkCapabilities {
                supports_streaming: true,
                supports_batching: false,
                supports_tools: true,
                max_context_length: None,
                modalities: vec!["text".to_string()],
            },
            cost_structure: None,
        })
        .expect("create sink");

        let mut ctx = make_ctx(vec![]);
        ctx.query = Some("beta=true&foo=bar".to_string());

        let url = sink.build_url(&ctx, Protocol::Anthropic).expect("url");
        assert_eq!(
            url.as_str(),
            "https://api.anthropic.com/v1/messages?beta=true&foo=bar"
        );
    }
}
