//! Sink trait and core implementations

use super::types::{
    CostStructure, ModelList, Protocol, ResponseChunk, SinkCapabilities, SinkHealth,
};
use crate::{
    Result,
    access::{IdentityContext, SubjectIdentity},
    tracing::CorrelationId,
};
use async_trait::async_trait;
use futures::Stream;
use http::HeaderMap;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

/// Stream of response chunks
pub type ResponseStream = Pin<Box<dyn Stream<Item = Result<ResponseChunk>> + Send>>;

/// Request context for request execution
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub identity: SubjectIdentity<RouterIdentityContext>,
    pub correlation_id: CorrelationId,
    /// Request headers supplied by the caller (not credentials)
    pub headers: HeaderMap,
    /// Raw query string from the incoming HTTP request (without leading '?')
    pub query: Option<String>,
    pub trace_id: Option<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

/// Identity context specific to router
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RouterIdentityContext {
    pub org_id: Option<String>,
    pub user_id: Option<String>,
    pub api_key_hash: Option<String>,
}

impl IdentityContext for RouterIdentityContext {
    fn to_attributes(&self) -> std::collections::HashMap<String, String> {
        let mut attrs = std::collections::HashMap::new();
        if let Some(ref org_id) = self.org_id {
            attrs.insert("org_id".to_string(), org_id.clone());
        }
        if let Some(ref user_id) = self.user_id {
            attrs.insert("user_id".to_string(), user_id.clone());
        }
        if let Some(ref api_key_hash) = self.api_key_hash {
            attrs.insert("api_key_hash".to_string(), api_key_hash.clone());
        }
        attrs
    }

    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "org_id" => self.org_id.as_deref(),
            "user_id" => self.user_id.as_deref(),
            "api_key_hash" => self.api_key_hash.as_deref(),
            _ => None,
        }
    }
}

/// Sink trait - anything that can receive and process requests
#[async_trait]
pub trait Sink: Send + Sync {
    /// Describe sink capabilities
    async fn describe(&self) -> SinkDescription;

    /// Check sink health
    async fn probe(&self) -> SinkHealth;

    /// Execute request
    async fn execute(
        &self,
        ctx: &RequestContext,
        request: super::types::RequestStream,
    ) -> Result<ResponseStream>;
}

/// Description of a sink's capabilities
#[derive(Debug, Clone)]
pub struct SinkDescription {
    pub id: String,
    pub accepted_protocols: Vec<Protocol>,
    pub models: ModelList,
    pub capabilities: SinkCapabilities,
    pub cost_structure: Option<CostStructure>,
}

impl SinkDescription {
    /// Check if this sink supports the given model
    pub fn supports_model(&self, model: &str) -> bool {
        match &self.models {
            ModelList::Static(models) => models.iter().any(|m| m == model),
            ModelList::Dynamic | ModelList::Infinite => true,
        }
    }

    /// Check if this sink accepts the given protocol
    pub fn accepts_protocol(&self, protocol: Protocol) -> bool {
        self.accepted_protocols.contains(&protocol)
    }
}
