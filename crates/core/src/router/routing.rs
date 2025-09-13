//! Router implementation for intelligent request routing

use super::executor::PlanExecutor;
use super::index::SinkIndex;
use super::middleware::Middleware;
use super::plan::{Route, RoutingPlan};
use super::registry::SinkRegistry;
use super::sink::{RequestContext, ResponseStream, Sink, SinkDescription};
use super::strategy::{RoutingStrategy, ScoredRoute, SimpleStrategy, SinkCandidate};
use super::types::{Protocol, RequestCapabilities, RequestDescriptor, RequestStream, RetryConfig};
use super::{SinkHealth, SinkSnapshot};
use crate::Result;
use crate::router::SinkCapabilities;
use crate::router::types::ModelList;
use crate::state::StateBackend;
use async_trait::async_trait;
use chrono::Utc;
use futures::future::BoxFuture;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

/// Router - makes routing decisions
pub struct Router {
    state_backend: Arc<dyn StateBackend>,
    sink_registry: Arc<SinkRegistry>,
    strategy: Box<dyn RoutingStrategy>,
    middleware: Vec<Arc<dyn Middleware>>,
    sink_index: Option<Arc<SinkIndex>>, // Optional fast-path index
}

impl Router {
    /// Create a new router builder
    pub fn builder() -> RouterBuilder {
        RouterBuilder::new()
    }

    /// Route a request to create a plan
    pub async fn route(
        &self,
        ctx: &RequestContext,
        desc: &RequestDescriptor,
    ) -> Result<RoutingPlan> {
        // Resolve aliases for model
        let concrete_models = self.resolve_model(&desc.model).await?;

        // Find eligible sinks (no protocol conversion in v2)
        let candidates = self
            .find_eligible_sinks(
                &concrete_models,
                desc.protocol,
                &desc.capabilities,
                desc.context_length_hint,
                self.sink_index.as_deref(),
            )
            .await?;

        if candidates.is_empty() {
            return Err(crate::Error::NoSinksAvailable);
        }

        // Apply routing strategy
        let scored_routes = self.strategy.evaluate(ctx, desc, candidates).await?;
        if scored_routes.is_empty() {
            return Err(crate::Error::NoSinksAvailable);
        }

        // Convert to routes
        let (primary, fallbacks) = self.create_routes(scored_routes)?;

        // Create plan
        Ok(RoutingPlan::new(ctx.clone(), primary, fallbacks))
    }

    /// Execute a routing plan
    pub async fn execute(
        &self,
        plan: RoutingPlan,
        request: RequestStream,
    ) -> Result<ResponseStream> {
        debug!("Executing routing plan: {:?}", self.sink_index);

        let executor = PlanExecutor::new(self.sink_registry.clone());

        // Build middleware pipeline around the executor
        let middlewares = self.middleware.clone();
        let ctx_arc = Arc::new(plan.context.clone());

        // Terminal handler
        let terminal = move |req: RequestStream| {
            let executor = executor;
            let plan = plan;
            Box::pin(async move { executor.execute(plan, req).await })
                as BoxFuture<'static, Result<ResponseStream>>
        };

        // Build chain from back to front
        let mut next: super::middleware::Next = Box::new(terminal);
        for mw in middlewares.into_iter().rev() {
            let prev_next = next;
            let ctx_iter = ctx_arc.clone();
            next = Box::new(move |req_stream| {
                let mw = mw.clone();
                Box::pin(async move {
                    let mut ctx_clone = (*ctx_iter).clone();
                    mw.process(&mut ctx_clone, req_stream, prev_next).await
                })
            });
        }

        // Kick off the chain
        next(request).await
    }
    /// Refresh the attached sink index from the current registry. Returns number of refreshed sinks.
    pub async fn refresh_index(&self) -> Result<usize> {
        if let Some(index) = &self.sink_index {
            let n = index.refresh_from_registry(&self.sink_registry).await;
            Ok(n)
        } else {
            Ok(0)
        }
    }

    /// Resolve model aliases to concrete models
    async fn resolve_model(&self, model: &str) -> Result<Vec<String>> {
        // First check if it's an alias
        match self.state_backend.resolve_model_alias(model).await {
            Ok(models) if !models.is_empty() => Ok(models),
            _ => Ok(vec![model.to_string()]), // Use as-is if not an alias
        }
    }

    /// Find eligible sinks for the given models and protocol
    async fn find_eligible_sinks(
        &self,
        models: &[String],
        protocol: Protocol,
        req_caps: &RequestCapabilities,
        context_hint: Option<usize>,
        index: Option<&SinkIndex>,
    ) -> Result<Vec<SinkCandidate>> {
        let mut candidates = Vec::new();

        if let Some(index) = index {
            // Use snapshots for hot path
            let snapshots = index.list().await;
            for (
                sink_id,
                SinkSnapshot {
                    description,
                    health,
                    ..
                },
            ) in snapshots
            {
                let Some(sink) = self.sink_registry.get(&sink_id).await else {
                    continue;
                };

                // Check if sink is healthy
                if !health.healthy {
                    continue;
                }

                // Check protocol support (no conversion in v2)
                if !description.accepts_protocol(protocol) {
                    continue;
                }

                // Check capability support
                if req_caps.needs_streaming && !description.capabilities.supports_streaming {
                    continue;
                }

                if req_caps.needs_tools && !description.capabilities.supports_tools {
                    continue;
                }

                // Modalities (must include all requested)
                let sink_modalities: HashSet<_> = description
                    .capabilities
                    .modalities
                    .iter()
                    .cloned()
                    .collect();

                let req_modalities: HashSet<_> = req_caps.modalities.iter().cloned().collect();
                if !req_modalities.is_subset(&sink_modalities) {
                    continue;
                }

                // Context length best-effort check
                if let (Some(max_ctx), Some(input_hint)) =
                    (description.capabilities.max_context_length, context_hint)
                {
                    let want_out = req_caps.max_tokens.unwrap_or(0) as usize;
                    if input_hint + want_out > max_ctx {
                        continue;
                    }
                }

                candidates.push(SinkCandidate {
                    sink: sink.clone(),
                    description,
                    health,
                    needs_conversion: None,
                });
            }
        } else {
            // Fallback: query sinks directly (slower)
            let sinks = self.sink_registry.get_all().await;

            for sink in sinks {
                let description = sink.describe().await;
                let health = sink.probe().await;

                if !health.healthy {
                    continue;
                }

                if !description.accepts_protocol(protocol) {
                    continue;
                }

                if req_caps.needs_streaming && !description.capabilities.supports_streaming {
                    continue;
                }
                if req_caps.needs_tools && !description.capabilities.supports_tools {
                    continue;
                }
                let sink_modalities: HashSet<_> = description
                    .capabilities
                    .modalities
                    .iter()
                    .cloned()
                    .collect();
                let req_modalities: HashSet<_> = req_caps.modalities.iter().cloned().collect();
                if !req_modalities.is_subset(&sink_modalities) {
                    continue;
                }
                if let (Some(max_ctx), Some(input_hint)) =
                    (description.capabilities.max_context_length, context_hint)
                {
                    let want_out = req_caps.max_tokens.unwrap_or(0) as usize;
                    if input_hint + want_out > max_ctx {
                        continue;
                    }
                }

                candidates.push(SinkCandidate {
                    sink: sink.clone(),
                    description,
                    health,
                    needs_conversion: None,
                });
            }
        }

        Ok(candidates)
    }

    /// Create routes from scored routes
    fn create_routes(&self, mut scored: Vec<ScoredRoute>) -> Result<(Route, Vec<Route>)> {
        if scored.is_empty() {
            return Err(crate::Error::NoSinksAvailable);
        }

        // Sort by score (highest first)
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));

        let primary = scored.remove(0);
        let primary_route = Route {
            sink_id: primary.sink_id,
            protocol_conversion: primary.conversion_needed,
            timeout: Duration::from_secs(300),
            retry_config: RetryConfig::default(),
        };

        let fallback_routes = scored
            .into_iter()
            .take(2) // Limit fallbacks
            .map(|s| Route {
                sink_id: s.sink_id,
                protocol_conversion: s.conversion_needed,
                timeout: Duration::from_secs(30),
                retry_config: RetryConfig::default(),
            })
            .collect();

        Ok((primary_route, fallback_routes))
    }
}

// Router also implements Sink for composability
#[async_trait]
impl Sink for Router {
    async fn describe(&self) -> SinkDescription {
        // Aggregate descriptions from all sinks
        let sinks = self.sink_registry.get_all().await;
        let mut all_protocols = Vec::new();
        let mut supports_streaming = false;
        let mut supports_tools = false;
        let mut max_context = 0usize;

        for sink in sinks {
            let desc = sink.describe().await;
            for proto in desc.accepted_protocols {
                if !all_protocols.contains(&proto) {
                    all_protocols.push(proto);
                }
            }
            supports_streaming |= desc.capabilities.supports_streaming;
            supports_tools |= desc.capabilities.supports_tools;
            if let Some(ctx_len) = desc.capabilities.max_context_length {
                max_context = max_context.max(ctx_len);
            }
        }

        SinkDescription {
            id: "router".to_string(),
            accepted_protocols: all_protocols,
            capabilities: SinkCapabilities {
                supports_streaming,
                supports_batching: false,
                supports_tools,
                max_context_length: Some(max_context),
                modalities: vec!["text".to_string()],
            },
            cost_structure: None,
        }
    }

    async fn probe(&self) -> SinkHealth {
        // Aggregate health from all sinks
        let sinks = self.sink_registry.get_all().await;
        let mut healthy_count = 0;
        let mut total_count = 0;

        for sink in sinks {
            let health = sink.probe().await;
            total_count += 1;
            if health.healthy {
                healthy_count += 1;
            }
        }

        SinkHealth {
            healthy: healthy_count > 0,
            latency_ms: None,
            error_rate: if total_count > 0 {
                1.0 - (healthy_count as f32 / total_count as f32)
            } else {
                1.0
            },
            last_error: None,
            last_check: Utc::now(),
        }
    }

    async fn execute(
        &self,
        _ctx: &RequestContext,
        _request: RequestStream,
    ) -> Result<ResponseStream> {
        // Router-as-sink execution path requires request peeking to build a descriptor.
        // Not yet implemented in v2 core.
        Err(crate::Error::Internal(
            "Router used as a sink: not implemented".into(),
        ))
    }
}

/// Builder for Router
pub struct RouterBuilder {
    state_backend: Option<Arc<dyn StateBackend>>,
    sink_registry: Option<Arc<SinkRegistry>>,
    strategy: Option<Box<dyn RoutingStrategy>>,
    middleware: Vec<Arc<dyn Middleware>>,
    sink_index: Option<Arc<SinkIndex>>,
}

impl RouterBuilder {
    /// Create a new router builder
    pub fn new() -> Self {
        Self {
            state_backend: None,
            sink_registry: None,
            strategy: None,
            middleware: Vec::new(),
            sink_index: None,
        }
    }

    /// Set the state backend
    pub fn state_backend(mut self, backend: Arc<dyn StateBackend>) -> Self {
        self.state_backend = Some(backend);
        self
    }

    /// Set the sink registry
    pub fn sink_registry(mut self, registry: Arc<SinkRegistry>) -> Self {
        self.sink_registry = Some(registry);
        self
    }

    /// Set the routing strategy
    pub fn strategy(mut self, strategy: Box<dyn RoutingStrategy>) -> Self {
        self.strategy = Some(strategy);
        self
    }

    /// Add middleware
    pub fn middleware(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.middleware.push(middleware);
        self
    }

    /// Set an optional sink index to use for fast routing
    pub fn sink_index(mut self, index: Arc<SinkIndex>) -> Self {
        self.sink_index = Some(index);
        self
    }

    /// Build the router
    pub fn build(self) -> Router {
        Router {
            state_backend: self.state_backend.expect("state_backend is required"),
            sink_registry: self
                .sink_registry
                .unwrap_or_else(|| Arc::new(SinkRegistry::new())),
            strategy: self
                .strategy
                .unwrap_or_else(|| Box::new(SimpleStrategy::new())),
            middleware: self.middleware,
            sink_index: self.sink_index,
        }
    }
}

impl Default for RouterBuilder {
    fn default() -> Self {
        Self::new()
    }
}
