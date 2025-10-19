//! Mock connector implementations for testing

use crate::Result;
use crate::router::connector::{Connector, ConnectorDescription, RequestContext};
use crate::router::types::RequestStream;
use crate::router::types::{ConnectorCapabilities, ConnectorHealth, Protocol, ResponseChunk};
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::json;
use std::pin::Pin;

pub struct MockConnector {
    pub id: String,
    pub accepted_protocols: Vec<Protocol>,
    pub capabilities: ConnectorCapabilities,
    pub healthy: bool,
}

impl MockConnector {
    pub fn success(id: &str) -> Self {
        Self {
            id: id.to_string(),
            accepted_protocols: vec![Protocol::OpenAIChat, Protocol::Anthropic],
            capabilities: ConnectorCapabilities {
                supports_streaming: true,
                supports_batching: false,
                supports_tools: true,
                max_context_length: Some(128_000),
                modalities: vec!["text".into()],
            },
            healthy: true,
        }
    }

    pub fn unhealthy(id: &str) -> Self {
        let mut s = Self::success(id);
        s.healthy = false;
        s
    }
}

#[async_trait]
impl Connector for MockConnector {
    async fn describe(&self) -> ConnectorDescription {
        ConnectorDescription {
            id: self.id.clone(),
            accepted_protocols: self.accepted_protocols.clone(),
            capabilities: self.capabilities.clone(),
            cost_structure: None,
        }
    }

    async fn probe(&self) -> ConnectorHealth {
        ConnectorHealth {
            healthy: self.healthy,
            latency_ms: Some(50),
            error_rate: if self.healthy { 0.0 } else { 1.0 },
            last_error: None,
            last_check: chrono::Utc::now(),
        }
    }

    async fn execute(
        &self,
        _ctx: &RequestContext,
        mut request: RequestStream,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<ResponseChunk>> + Send>>> {
        // Consume the request and echo a simple response
        let mut messages = Vec::new();
        while let Some(item) = request.next().await {
            let v = item?;
            messages.push(v);
        }

        let stream = async_stream::stream! {
            yield Ok(ResponseChunk::Headers(Default::default()));
            yield Ok(ResponseChunk::Content(json!({"ok": true, "echo": messages })));
            yield Ok(ResponseChunk::Stop { reason: crate::router::types::StopReason::Complete, error: None, cost: None });
        };
        Ok(Box::pin(stream))
    }
}
