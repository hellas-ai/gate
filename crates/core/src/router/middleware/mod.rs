//! Middleware system for request/response processing

mod cost_tracker;
mod key_capture;
mod monitor;
mod rate_limit;

pub use cost_tracker::CostTrackerMiddleware;
pub use key_capture::{KeyCaptureMiddleware, KeyCaptureRegistrar};
pub use monitor::MonitoringMiddleware;
pub use rate_limit::RateLimitMiddleware;

use crate::Result;
use async_trait::async_trait;
use futures::Stream;
use futures::future::BoxFuture;
use std::pin::Pin;

/// Stream of request data
pub type RequestStream = super::types::RequestStream;

/// Stream of response data
pub type ResponseStream = Pin<Box<dyn Stream<Item = Result<super::types::ResponseChunk>> + Send>>;

/// Next middleware in the chain
pub type Next = Box<dyn FnOnce(RequestStream) -> BoxFuture<'static, Result<ResponseStream>> + Send>;

/// Middleware trait for processing requests and responses
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Process the request/response through this middleware
    async fn process(
        &self,
        ctx: &mut super::sink::RequestContext,
        request: RequestStream,
        next: Next,
    ) -> Result<ResponseStream>;
}
