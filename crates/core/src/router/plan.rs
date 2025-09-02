//! Routing plan definition and execution

use super::protocols::ProtocolConversion;
use super::sink::{RequestContext, ResponseStream, Sink};
use super::types::{RequestStream, ResponseChunk, RetryConfig, StopReason};
use crate::Result;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// Executable routing plan
#[derive(Debug, Clone)]
pub struct RoutingPlan {
    pub id: uuid::Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub context: RequestContext,
    pub primary_route: Route,
    pub fallback_routes: Vec<Route>,
    pub estimated_cost: Option<Decimal>,
    pub routing_rationale: String,
}

/// A single route in the plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub sink_id: String,
    pub protocol_conversion: Option<ProtocolConversion>,
    pub timeout: Duration,
    pub retry_config: RetryConfig,
}

impl RoutingPlan {
    /// Create a new routing plan
    pub fn new(context: RequestContext, primary_route: Route, fallback_routes: Vec<Route>) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            created_at: chrono::Utc::now(),
            context,
            primary_route,
            fallback_routes,
            estimated_cost: None,
            routing_rationale: "Default routing strategy".to_string(),
        }
    }

    /// Set estimated cost
    pub fn with_estimated_cost(mut self, cost: Decimal) -> Self {
        self.estimated_cost = Some(cost);
        self
    }

    /// Set routing rationale
    pub fn with_rationale(mut self, rationale: String) -> Self {
        self.routing_rationale = rationale;
        self
    }
}

/// Executor for routing plans
pub struct PlanExecutor {
    sink_registry: Arc<super::routing::SinkRegistry>,
}

impl PlanExecutor {
    /// Create a new plan executor
    pub fn new(sink_registry: Arc<super::routing::SinkRegistry>) -> Self {
        Self { sink_registry }
    }

    /// Execute a routing plan
    pub async fn execute(
        &self,
        plan: RoutingPlan,
        request: RequestStream,
    ) -> Result<ResponseStream> {
        // Try primary route
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
                // Fallbacks require a fresh request stream; returning the primary error for now
                Err(primary_err)
            }
        }
    }

    /// Execute a single route
    async fn execute_route(
        &self,
        ctx: &RequestContext,
        request: RequestStream,
        route: &Route,
    ) -> Result<ResponseStream> {
        // Get the sink
        let sink = self
            .sink_registry
            .get(&route.sink_id)
            .await
            .ok_or_else(|| crate::Error::Internal(format!("Sink not found: {}", route.sink_id)))?;

        // No protocol conversion in v2 core
        if let Some(ref conversion) = route.protocol_conversion {
            return Err(crate::Error::UnsupportedConversion(
                format!("{:?}", conversion.from),
                format!("{:?}", conversion.to),
            ));
        }

        // Execute with retries
        self.execute_with_retries(ctx, sink, request, &route.retry_config, route.timeout)
            .await
    }

    /// Execute with retry logic
    async fn execute_with_retries(
        &self,
        ctx: &RequestContext,
        sink: Arc<dyn Sink>,
        request: RequestStream,
        _retry_config: &RetryConfig,
        timeout: Duration,
    ) -> Result<ResponseStream> {
        // KISS: Single attempt for streaming requests
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
