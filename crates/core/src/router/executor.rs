use super::plan::{Route, RoutingPlan};
use super::registry::SinkRegistry;
use super::sink::{RequestContext, Sink};
use super::types::{RequestStream, ResponseChunk, StopReason};
use crate::Result;
use std::sync::Arc;
use std::time::Duration;

pub struct PlanExecutor {
    sink_registry: Arc<SinkRegistry>,
}

impl PlanExecutor {
    pub fn new(sink_registry: Arc<SinkRegistry>) -> Self {
        Self { sink_registry }
    }

    pub async fn execute(
        &self,
        plan: RoutingPlan,
        request: RequestStream,
    ) -> Result<super::sink::ResponseStream> {
        let result = self
            .execute_route(&plan.context, request, &plan.primary_route)
            .await;
        match result {
            Ok(stream) => Ok(stream),
            Err(primary_err) => {
                #[cfg(feature = "tracing")]
                {
                    tracing::warn!(
                        "Primary route failed: {} - {}",
                        plan.primary_route.sink_id,
                        primary_err
                    );
                }
                Err(primary_err)
            }
        }
    }

    async fn execute_route(
        &self,
        ctx: &RequestContext,
        request: RequestStream,
        route: &Route,
    ) -> Result<super::sink::ResponseStream> {
        let sink = self
            .sink_registry
            .get(&route.sink_id)
            .await
            .ok_or_else(|| crate::Error::Internal(format!("Sink not found: {}", route.sink_id)))?;

        if route.protocol_conversion.is_some() {
            return Err(crate::Error::UnsupportedConversion(
                "from".into(),
                "to".into(),
            ));
        }

        self.execute_with_retries(ctx, sink, request, &route.retry_config, route.timeout)
            .await
    }

    async fn execute_with_retries(
        &self,
        ctx: &RequestContext,
        sink: Arc<dyn Sink>,
        request: RequestStream,
        _retry_config: &super::types::RetryConfig,
        timeout: Duration,
    ) -> Result<super::sink::ResponseStream> {
        match tokio::time::timeout(timeout, sink.execute(ctx, request)).await {
            Ok(Ok(stream)) => Ok(stream),
            Ok(Err(err)) => Err(err),
            Err(_) => {
                let chunk = ResponseChunk::Stop {
                    reason: StopReason::Timeout,
                    error: Some("Request timed out".to_string()),
                    cost: None,
                };
                let stream = futures::stream::once(async move { Ok(chunk) });
                Ok(Box::pin(stream))
            }
        }
    }
}
