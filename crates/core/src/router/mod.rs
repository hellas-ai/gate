//! Smart routing module for LLM inference requests
//!
//! This module provides intelligent routing of inference requests across multiple
//! providers, protocols, and deployment contexts (WASM, local daemon, Cloudflare Workers).

pub mod connector;
pub mod connectors;
pub mod credentials;
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
pub mod signals;
pub mod strategy;
pub mod transport;
pub mod types;

#[cfg(test)]
mod tests;

// Re-export main types
pub use connector::RequestContext;
pub use connector::{Connector, ConnectorDescription, ResponseStream};
pub use index::{ConnectorIndex, ConnectorSnapshot};
pub use plan::{Route, RoutingPlan};
pub use registry::ConnectorRegistry;
pub use routing::Router;
pub use types::{
    ActualCost, ConnectorCapabilities, ConnectorHealth, ModelCapabilities, Protocol, ResponseChunk,
    StopReason, VirtualModel,
};
