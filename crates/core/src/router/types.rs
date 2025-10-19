//! Core types for the router module

use futures::Stream;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::pin::Pin;
use std::time::Duration;

/// Protocol identifier for different LLM APIs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Protocol {
    OpenAIMessages,    // v1/chat/messages
    OpenAIChat,        // v1/chat/completions
    OpenAICompletions, // v1/completions
    OpenAIResponses,   // v1/responses
    Anthropic,         // v1/messages
    Unknown,           // Escape hatch
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::OpenAIMessages => write!(f, "OpenAI Messages"),
            Protocol::OpenAIChat => write!(f, "OpenAI Chat"),
            Protocol::OpenAICompletions => write!(f, "OpenAI Completions"),
            Protocol::OpenAIResponses => write!(f, "OpenAI Responses"),
            Protocol::Anthropic => write!(f, "Anthropic"),
            Protocol::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Connector health information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorHealth {
    pub healthy: bool,
    pub latency_ms: Option<u64>,
    pub error_rate: f32,
    pub last_error: Option<String>,
    pub last_check: chrono::DateTime<chrono::Utc>,
}

/// Virtual model definition for user-defined routing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualModel {
    pub name: String,
    pub version: String,
    pub owner: String,
    pub routing_rules: RoutingRules,
    pub visibility: Visibility,
}

/// Routing rules for virtual models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRules {
    pub strategy: String,
    pub targets: Vec<RoutingTarget>,
    pub fallback: Option<Box<RoutingRules>>,
    pub constraints: Option<RoutingConstraints>,
}

/// Target for routing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingTarget {
    pub model: String,
    pub weight: Option<f64>,
    pub priority: Option<u32>,
    pub constraints: Option<RoutingConstraints>,
}

/// Constraints for routing decisions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConstraints {
    pub max_cost: Option<Decimal>,
    pub max_latency_ms: Option<u64>,
    pub required_capabilities: Option<Vec<String>>,
    pub excluded_providers: Option<Vec<String>>,
}

/// Visibility of virtual models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
    Organization(String),
}

/// Model capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub context_length: usize,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_streaming: bool,
    pub modalities: Vec<String>,
}

/// Connector capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorCapabilities {
    pub supports_streaming: bool,
    pub supports_batching: bool,
    pub supports_tools: bool,
    pub max_context_length: Option<usize>,
    pub modalities: Vec<String>,
}

/// Actual cost from execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActualCost {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub input_cost_usd: Decimal,
    pub output_cost_usd: Decimal,
    pub total_cost_usd: Decimal,
    pub cached_input_tokens: Option<u32>,
    pub provider_metadata: HashMap<String, JsonValue>,
}

/// Response chunk for streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseChunk {
    /// Response headers (optional first chunk)
    Headers(HashMap<String, String>),
    /// Protocol-agnostic content
    Content(JsonValue),
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
    },
    Metadata(HashMap<String, JsonValue>),
    Stop {
        reason: StopReason,
        error: Option<String>,
        /// Final accumulated cost if available
        cost: Option<ActualCost>,
    },
}

/// Reason for stopping generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StopReason {
    Complete,
    MaxTokens,
    StopSequence(String),
    ToolUse,
    Error,
    Cancelled,
    Timeout,
}

/// Cost structure for a sink
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostStructure {
    pub input_cost_per_token: Decimal,
    pub output_cost_per_token: Decimal,
    pub cached_input_cost_per_token: Option<Decimal>,
    pub currency: String,
}

/// List of models a connector supports
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelList {
    /// Static list of known models
    Static(Vec<String>),
    /// Models are discovered dynamically
    Dynamic,
    /// Connector accepts any model name
    Infinite,
}

/// Retry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub exponential_base: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            exponential_base: 2.0,
        }
    }
}

/// Quota behavior when limits are exceeded
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuotaBehavior {
    /// Reject the request
    Reject,
    /// Allow but warn
    WarnOnly,
    /// Allow and track overage
    TrackOverage,
}

/// Request capabilities extracted from the request
#[derive(Debug, Clone)]
pub struct RequestCapabilities {
    pub needs_tools: bool,
    pub needs_vision: bool,
    pub needs_streaming: bool,
    pub max_tokens: Option<u32>,
    pub modalities: Vec<String>,
}

/// Stream of request items with its protocol
pub struct RequestStream {
    protocol: Protocol,
    inner: Pin<Box<dyn Stream<Item = crate::Result<JsonValue>> + Send>>,
}

impl RequestStream {
    pub fn new(
        protocol: Protocol,
        inner: Pin<Box<dyn Stream<Item = crate::Result<JsonValue>> + Send>>,
    ) -> Self {
        Self { protocol, inner }
    }

    pub fn protocol(&self) -> Protocol {
        self.protocol
    }
}

impl Stream for RequestStream {
    type Item = crate::Result<JsonValue>;
    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        // Safety: we're not moving inner
        let this = unsafe { self.get_unchecked_mut() };
        Pin::new(&mut this.inner).poll_next(cx)
    }
}

/// Descriptor for routing without consuming the request stream
#[derive(Debug, Clone)]
pub struct RequestDescriptor {
    pub model: String,
    pub protocol: Protocol,
    pub capabilities: RequestCapabilities,
    /// Optional hint of total input tokens to enforce context length
    pub context_length_hint: Option<usize>,
}
