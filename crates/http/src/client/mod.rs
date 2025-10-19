//! Gate HTTP client

pub mod auth;
pub mod clients;
pub mod config;
pub mod error;
pub mod inference;

pub use clients::{AuthenticatedGateClient, ClientBuilder, PublicGateClient};
pub use error::ClientError;
