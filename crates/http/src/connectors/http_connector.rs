//! Generic HTTP-based connector for external LLM providers

use crate::connectors::anthropic::{
    ANTHROPIC_BETA, ANTHROPIC_BETA_OAUTH, ANTHROPIC_VERSION, ANTHROPIC_VERSION_VALUE,
    CLAUDE_CODE_USER_AGENT, X_API_KEY, X_APP, X_APP_VALUE,
};

use super::adapters::HttpEndpointAdapter;
use super::credentials::{HeaderPassthroughResolver, StaticKeyResolver};
use super::transport::HttpTransport;
use async_trait::async_trait;
use futures::StreamExt;
use gate_core::router::connector::{
    Connector, ConnectorDescription, RequestContext, ResponseStream,
};
use gate_core::router::credentials::CredentialResolver;
// use gate_core::router::protocols::ProtocolAdapter; // adapters through HttpEndpointAdapter
use gate_core::router::transport::Transport;
use gate_core::router::types::{
    ConnectorCapabilities, ConnectorHealth, CostStructure, Protocol, RequestStream, ResponseChunk,
    StopReason,
};
use gate_core::{Error, Result};
use http::HeaderMap as HttpHeaderMap;
use http::header::{AUTHORIZATION, USER_AGENT};
use http::{HeaderName, HeaderValue};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use url::{Url, form_urlencoded};

/// Provider type for HTTP connectors
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    OpenAI,
    OpenAICodex,
    Custom,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::Anthropic => write!(f, "anthropic"),
            Provider::OpenAI => write!(f, "openai"),
            Provider::OpenAICodex => write!(f, "openai-codex"),
            Provider::Custom => write!(f, "custom"),
        }
    }
}

/// Configuration for an HTTP connector
#[derive(Debug, Clone)]
pub struct HttpConnectorConfig {
    pub id: String,
    pub provider: Provider,
    pub base_url: String,
    pub api_key: Option<String>,
    pub models: Vec<String>,
    pub timeout: Duration,
    pub max_retries: u32,
    pub accepted_protocols: Vec<Protocol>,
    pub capabilities: ConnectorCapabilities,
    pub cost_structure: Option<CostStructure>,
    pub allow_passthrough: bool,
}

/// HTTP-based connector for external LLM providers
pub struct HttpConnector {
    config: HttpConnectorConfig,
    client: Client,
    health: Arc<RwLock<ConnectorHealth>>,
    resolvers: Vec<Arc<dyn CredentialResolver>>,
    adapters: HashMap<Protocol, Arc<dyn HttpEndpointAdapter>>,
}

impl HttpConnector {
    /// Create a new HTTP connector
    pub fn new(config: HttpConnectorConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| Error::Internal(format!("Failed to create HTTP client: {e}")))?;

        let health = Arc::new(RwLock::new(ConnectorHealth {
            healthy: true,
            latency_ms: None,
            error_rate: 0.0,
            last_error: None,
            last_check: chrono::Utc::now(),
        }));

        // Build credential resolvers (static first, then passthrough)
        let mut resolvers: Vec<Arc<dyn CredentialResolver>> = Vec::new();
        if let Some(ref key) = config.api_key {
            resolvers.push(Arc::new(StaticKeyResolver::new(
                config.provider.clone(),
                key.clone(),
            )));
        }
        if config.allow_passthrough {
            resolvers.push(Arc::new(HeaderPassthroughResolver::new(
                config.provider.clone(),
            )));
        }

        // Register protocol adapters (minimal set)
        let mut adapters: HashMap<Protocol, Arc<dyn HttpEndpointAdapter>> = HashMap::new();
        adapters.insert(
            Protocol::OpenAIChat,
            Arc::new(super::adapters::OpenAIChatAdapter),
        );
        adapters.insert(
            Protocol::OpenAIResponses,
            Arc::new(super::adapters::OpenAIResponsesAdapter),
        );
        adapters.insert(
            Protocol::Anthropic,
            Arc::new(super::adapters::AnthropicMessagesAdapter),
        );
        adapters.insert(
            Protocol::OpenAIMessages,
            Arc::new(super::adapters::OpenAIMessagesAdapter),
        );

        Ok(Self {
            config,
            client,
            health,
            resolvers,
            adapters,
        })
    }

    // auth inference now handled by resolvers in build_outgoing_headers

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

    // endpoint selection moved to adapters

    /// Build the upstream URL from base_url, endpoint, and forwarded query
    fn build_url(&self, ctx: &RequestContext, protocol: Protocol) -> Result<Url> {
        let adapter = self.adapters.get(&protocol).ok_or_else(|| {
            Error::InvalidRoutingConfig(format!("No adapter for protocol {protocol:?}"))
        })?;
        let endpoint = adapter
            .endpoint_path(&self.config.provider)
            .ok_or_else(|| {
                Error::InvalidRoutingConfig(format!(
                    "Provider {} doesn't support protocol {:?}",
                    self.config.provider, protocol
                ))
            })?;
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
        let request = self.get_first_request(&mut request_stream).await?;
        let protocol = request_stream.protocol();
        self.execute_via_transport(ctx, protocol, &request).await
    }

    /// Get and validate the first request from the stream
    async fn get_first_request(&self, stream: &mut RequestStream) -> Result<JsonValue> {
        stream
            .next()
            .await
            .ok_or_else(|| Error::InvalidRequest("Empty request stream".to_string()))?
    }

    /// Prepare the HTTP request with all necessary headers
    async fn build_outgoing_headers(&self, ctx: &RequestContext) -> HttpHeaderMap {
        use http::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST};
        let mut headers = HttpHeaderMap::new();

        // Authentication headers via resolvers
        for r in &self.resolvers {
            if let Some((hn, hv)) = r.resolve(ctx, &self.config.provider.to_string()).await {
                headers.insert(hn, hv);
                break;
            }
        }

        // Provider-required headers
        for (name, value) in self.provider_headers() {
            headers.insert(name, value);
        }

        // Forward selected client headers (no credentials or hop-by-hop)
        for (name, value) in ctx.headers.iter() {
            if name == HOST
                || name == CONTENT_LENGTH
                || name == CONTENT_TYPE
                || name == AUTHORIZATION
                || name == X_API_KEY
            {
                continue;
            }
            headers.insert(name.clone(), value.clone());
        }

        headers
    }

    async fn execute_via_transport(
        &self,
        ctx: &RequestContext,
        protocol: Protocol,
        request: &JsonValue,
    ) -> Result<ResponseStream> {
        // Build URL and headers
        let url = self.build_url(ctx, protocol)?;
        let headers = self.build_outgoing_headers(ctx).await;
        // Build transport request
        let treq = gate_core::router::transport::TransportRequest {
            url: url.as_str().to_string(),
            headers,
            body: request.clone(),
            timeout: Some(self.config.timeout),
        };
        let transport = HttpTransport::new(self.client.clone());
        let resp = transport.post_json(treq).await?;

        // Convert headers to a typed map for adapter detection
        let mut header_map = HttpHeaderMap::new();
        for (k, v) in &resp.headers {
            if let (Ok(name), Ok(val)) = (
                http::HeaderName::try_from(k.as_str()),
                http::HeaderValue::try_from(v.as_str()),
            ) {
                header_map.insert(name, val);
            }
        }
        let is_streaming = if let Some(adapter) = self.adapters.get(&protocol) {
            adapter.is_streaming_response(&header_map)
        } else {
            header_map
                .get(http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|ct| ct.contains("text/event-stream"))
                .unwrap_or(false)
        };

        if is_streaming {
            // SSE stream → chunks
            let provider = self.config.provider.clone();
            let stream = super::sse_parser::parse_sse(resp.body).map(move |result| match result {
                Ok(event) => {
                    if event.data == "[DONE]" {
                        Ok(ResponseChunk::Stop {
                            reason: StopReason::Complete,
                            error: None,
                            cost: None,
                        })
                    } else if let Ok(json) = serde_json::from_str::<JsonValue>(&event.data) {
                        if let Some(error) = json.get("error") {
                            Ok(ResponseChunk::Stop {
                                reason: StopReason::Error,
                                error: Some(error.to_string()),
                                cost: None,
                            })
                        } else {
                            Ok(ResponseChunk::Content(json))
                        }
                    } else {
                        Ok(ResponseChunk::Content(JsonValue::String(event.data)))
                    }
                }
                Err(e) => {
                    error!("SSE parse error from {}: {}", provider, e);
                    Ok(ResponseChunk::Stop {
                        reason: StopReason::Error,
                        error: Some(format!("Stream error: {e}")),
                        cost: None,
                    })
                }
            });
            Ok(Box::pin(stream))
        } else {
            // Accumulate body → JSON
            use futures::StreamExt;
            let mut body = Vec::new();
            let mut s = resp.body;
            while let Some(chunk) = s.next().await {
                let bytes: Vec<u8> = chunk?;
                body.extend_from_slice(&bytes);
            }
            let text = String::from_utf8_lossy(&body).to_string();
            let json: JsonValue = serde_json::from_str(&text).unwrap_or(JsonValue::String(text));
            let headers = resp.headers;
            let chunks = vec![
                Ok(ResponseChunk::Headers(headers)),
                Ok(ResponseChunk::Content(json)),
                Ok(ResponseChunk::Stop {
                    reason: StopReason::Complete,
                    error: None,
                    cost: None,
                }),
            ];
            Ok(Box::pin(futures::stream::iter(chunks)))
        }
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
impl Connector for HttpConnector {
    async fn describe(&self) -> ConnectorDescription {
        ConnectorDescription {
            id: self.config.id.clone(),
            accepted_protocols: self.config.accepted_protocols.clone(),
            capabilities: self.config.capabilities.clone(),
            cost_structure: self.config.cost_structure.clone(),
        }
    }

    async fn probe(&self) -> ConnectorHealth {
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
    use gate_core::router::connector::RouterIdentityContext;
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
    async fn test_passthrough_headers_anthropic() {
        let sink = HttpConnector::new(HttpConnectorConfig {
            id: "provider://anthropic".into(),
            provider: Provider::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            api_key: None,
            models: vec![],
            timeout: std::time::Duration::from_secs(5),
            max_retries: 0,
            accepted_protocols: vec![Protocol::Anthropic],
            capabilities: ConnectorCapabilities {
                supports_streaming: true,
                supports_batching: false,
                supports_tools: true,
                max_context_length: None,
                modalities: vec!["text".to_string()],
            },
            cost_structure: None,
            allow_passthrough: true,
        })
        .expect("create connector");

        // x-api-key preferred
        let ctx = make_ctx(vec![("x-api-key", "sk-ant-abc123")]);
        // Build headers via resolver chain
        let headers = sink.build_outgoing_headers(&ctx).await;
        assert_eq!(
            headers.get(X_API_KEY).unwrap(),
            &http::HeaderValue::from_static("sk-ant-abc123")
        );

        // Authorization Bearer fallback
        let ctx = make_ctx(vec![("authorization", "Bearer sk-ant-oat01-xyz")]);
        let headers = sink.build_outgoing_headers(&ctx).await;
        assert_eq!(
            headers.get(AUTHORIZATION).unwrap(),
            &http::HeaderValue::from_static("Bearer sk-ant-oat01-xyz")
        );
    }

    #[tokio::test]
    async fn test_build_url_forwards_query() {
        let sink = HttpConnector::new(HttpConnectorConfig {
            id: "provider://anthropic".into(),
            provider: Provider::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            api_key: None,
            models: vec![],
            timeout: std::time::Duration::from_secs(5),
            max_retries: 0,
            accepted_protocols: vec![Protocol::Anthropic],
            capabilities: ConnectorCapabilities {
                supports_streaming: true,
                supports_batching: false,
                supports_tools: true,
                max_context_length: None,
                modalities: vec!["text".to_string()],
            },
            cost_structure: None,
            allow_passthrough: true,
        })
        .expect("create connector");

        let mut ctx = make_ctx(vec![]);
        ctx.query = Some("beta=true&foo=bar".to_string());

        let url = sink.build_url(&ctx, Protocol::Anthropic).expect("url");
        assert_eq!(
            url.as_str(),
            "https://api.anthropic.com/v1/messages?beta=true&foo=bar"
        );
    }
}
