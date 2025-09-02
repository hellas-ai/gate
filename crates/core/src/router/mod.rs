//! Smart routing module for LLM inference requests
//!
//! This module provides intelligent routing of inference requests across multiple
//! providers, protocols, and deployment contexts (WASM, local daemon, Cloudflare Workers).

pub mod executor;
pub mod index;
pub mod middleware;
pub mod plan;
pub mod prelude;
pub mod protocols;
pub mod record;
pub mod registry;
pub mod routing;
pub mod service;
pub mod sink;
pub mod sinks;
pub mod strategy;
pub mod types;

#[cfg(test)]
mod tests;

// Re-export main types
pub use index::{SinkIndex, SinkSnapshot};
pub use plan::{Route, RoutingPlan};
pub use registry::SinkRegistry;
pub use routing::Router;
pub use sink::RequestContext;
pub use sink::{ResponseStream, Sink, SinkDescription};
pub use types::{
    ActualCost, ModelCapabilities, Protocol, ResponseChunk, SinkCapabilities, SinkHealth,
    StopReason, VirtualModel,
};
