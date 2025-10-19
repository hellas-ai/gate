//! Gate HTTP module providing router and middleware functionality
//!
//! This module provides a flexible HTTP routing system with OpenAPI documentation support
//! and extensible middleware framework for building API servers.

#[cfg(feature = "server")]
#[macro_use]
extern crate tracing;

pub mod error;
pub mod types;

#[cfg(feature = "server")]
pub mod auth;
#[cfg(feature = "server")]
#[path = "config/mod.rs"]
pub mod config;
#[cfg(feature = "server")]
pub mod connectors;
#[cfg(feature = "server")]
pub mod middleware;
#[cfg(feature = "server")]
pub mod routes;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
pub mod services;
#[cfg(feature = "server")]
pub mod state;
#[cfg(feature = "server")]
pub mod streaming;

#[cfg(feature = "client")]
pub mod client;

pub use error::{HttpError, Result};

#[cfg(feature = "server")]
pub use state::AppState;

// Re-export commonly used types
#[cfg(feature = "server")]
pub use axum::{Json, extract, response};
