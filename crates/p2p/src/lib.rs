//! P2P networking functionality for Gate
//!
//! This crate provides P2P utilities using Iroh primitives directly.
#[macro_use]
extern crate tracing;

#[cfg(not(target_arch = "wasm32"))]
pub mod router;
#[cfg(not(target_arch = "wasm32"))]
pub mod stream;

// Re-export commonly used types
#[cfg(not(target_arch = "wasm32"))]
pub use router::{RouterConfig, create_router};

// Re-export iroh types that are part of our public API
#[cfg(not(target_arch = "wasm32"))]
pub use iroh::{
    Endpoint, NodeAddr, NodeId, PublicKey, SecretKey, discovery,
    protocol::{ProtocolHandler, Router},
};
