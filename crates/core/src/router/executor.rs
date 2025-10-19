use super::connector::{Connector, RequestContext};
use super::plan::{Route, RoutingPlan};
use super::registry::ConnectorRegistry;
use super::types::{RequestStream, ResponseChunk, StopReason};
use crate::Result;
use std::sync::Arc;
use std::time::Duration;

pub struct PlanExecutor {
    connector_registry: Arc<ConnectorRegistry>,
}

impl PlanExecutor {
    pub fn new(connector_registry: Arc<ConnectorRegistry>) -> Self {
        Self { connector_registry }
    }

    pub async fn execute(
        &self,
        plan: RoutingPlan,
        request: RequestStream,
    ) -> Result<super::connector::ResponseStream> {
        let result = self
            .execute_route(&plan.context, request, &plan.primary_route)
            .await;
        match result {
            Ok(stream) => Ok(stream),
            Err(primary_err) => {
                #[cfg(feature = "tracing")]
                {
                    warn!(
                        "Primary route failed: {} - {}",
                        plan.primary_route.connector_id, primary_err
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
    ) -> Result<super::connector::ResponseStream> {
        let connector = self
            .connector_registry
            .get(&route.connector_id)
            .await
            .ok_or_else(|| {
                crate::Error::Internal(format!("Connector not found: {}", route.connector_id))
            })?;

        if route.protocol_conversion.is_some() {
            return Err(crate::Error::UnsupportedConversion(
                "from".into(),
                "to".into(),
            ));
        }

        self.execute_with_retries(ctx, connector, request, &route.retry_config, route.timeout)
            .await
    }

    async fn execute_with_retries(
        &self,
        ctx: &RequestContext,
        connector: Arc<dyn Connector>,
        request: RequestStream,
        _retry_config: &super::types::RetryConfig,
        timeout: Duration,
    ) -> Result<super::connector::ResponseStream> {
        match tokio::time::timeout(timeout, connector.execute(ctx, request)).await {
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
