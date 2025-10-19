//! Connector trait and core implementations

use super::types::{
    ConnectorCapabilities, ConnectorHealth, CostStructure, Protocol, ResponseChunk,
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
    pub headers: HeaderMap,
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

/// Connector trait - anything that can receive and process requests
#[async_trait]
pub trait Connector: Send + Sync {
    /// Describe connector capabilities
    async fn describe(&self) -> ConnectorDescription;

    /// Check connector health
    async fn probe(&self) -> ConnectorHealth;

    /// Execute request
    async fn execute(
        &self,
        ctx: &RequestContext,
        request: super::types::RequestStream,
    ) -> Result<ResponseStream>;
}

/// Description of a connector's capabilities
#[derive(Debug, Clone)]
pub struct ConnectorDescription {
    pub id: String,
    pub accepted_protocols: Vec<Protocol>,
    pub capabilities: ConnectorCapabilities,
    pub cost_structure: Option<CostStructure>,
}

impl ConnectorDescription {
    /// Check if this connector accepts the given protocol
    pub fn accepts_protocol(&self, protocol: Protocol) -> bool {
        self.accepted_protocols.contains(&protocol)
    }
}
