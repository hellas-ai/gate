//! Recording interfaces for request/response replay

use crate::Result;
use crate::router::sink::RequestContext;
use crate::router::types::ResponseChunk;
use async_trait::async_trait;
use std::collections::HashMap;

/// A recorded event in a request/response trace
#[derive(Debug, Clone)]
pub enum RecordEvent {
    RequestHeaders(HashMap<String, String>),
    RequestChunk(serde_json::Value),
    RouteSelected { sink_url: String, rationale: String },
    ResponseHeaders(HashMap<String, String>),
    ResponseChunk(ResponseChunk),
    Error(String),
}

/// Recorder trait for capturing request/response traces
#[async_trait]
pub trait Recorder: Send + Sync {
    async fn begin(&self, ctx: &RequestContext) -> Result<String>; // returns a trace id
    async fn record(&self, trace_id: &str, event: RecordEvent) -> Result<()>;
    async fn end(&self, trace_id: &str) -> Result<()>;
}

/// A no-op recorder implementation
pub struct NoopRecorder;

#[async_trait]
impl Recorder for NoopRecorder {
    async fn begin(&self, _ctx: &RequestContext) -> Result<String> {
        Ok(uuid::Uuid::new_v4().to_string())
    }
    async fn record(&self, _trace_id: &str, _event: RecordEvent) -> Result<()> {
        Ok(())
    }
    async fn end(&self, _trace_id: &str) -> Result<()> {
        Ok(())
    }
}
