//! HTTP-based connector implementations for external providers

pub mod adapters;
pub mod anthropic;
pub mod credentials;
pub mod http_connector;
pub mod openai;
pub mod response_converter;
pub mod sse_parser;
pub mod transport;

pub use http_connector::HttpConnector;

pub(crate) const DEFAULT_CONNECTOR_TIMEOUT_SECS: u64 = 600;
