//! HTTP-based sink implementations for external providers

pub mod anthropic;
pub mod http_sink;
pub mod openai;
pub mod response_converter;
pub mod sse_parser;

pub use http_sink::HttpSink;
