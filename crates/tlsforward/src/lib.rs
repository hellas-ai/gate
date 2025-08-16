//! Gate TLS forward service for HTTPS/TLS proxying over P2P connections
//!
//! This crate provides both client and server functionality:
//! - Server: Standalone TLS forward service that proxies HTTPS traffic
//! - Client: Library for gate servers to register with TLS forward service
#[macro_use]
extern crate tracing;

pub mod common;

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "client")]
pub use client::{CertificateManager, TlsAcceptorProvider, TlsForwardClient, TlsForwardHandler};

#[cfg(feature = "server")]
pub mod server;

// Re-export common types at crate root
pub use common::error;
pub use error::{Result, TlsForwardError};

/// ALPN protocol identifier for TLS forwarding over Iroh
pub const TLS_FORWARD_ALPN: &[u8] = b"/gate.tlsforward.v1.TlsForward/1.0";

/// ALPN protocol identifier for HTTP API over Iroh
pub const TLSFORWARD_HTTP_ALPN: &[u8] = b"/gate.tlsforward.v1.Http/1.0";
