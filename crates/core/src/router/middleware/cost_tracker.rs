//! Cost tracking middleware

use super::{Middleware, Next, RequestStream, ResponseStream};
use crate::router::connector::RequestContext;
use crate::router::types::{ActualCost, ResponseChunk};
use crate::state::StateBackend;
use crate::{Result, UsageRecord};
use async_trait::async_trait;
use futures::StreamExt;
use std::sync::Arc;

/// Cost tracking middleware
pub struct CostTrackerMiddleware<S: StateBackend + 'static> {
    state_backend: Arc<S>,
}

impl<S: StateBackend + 'static> CostTrackerMiddleware<S> {
    /// Create a new cost tracker middleware
    pub fn new(state_backend: Arc<S>) -> Self {
        Self { state_backend }
    }
}

#[async_trait]
impl<S: StateBackend + 'static> Middleware for CostTrackerMiddleware<S> {
    async fn process(
        &self,
        ctx: &mut RequestContext,
        request: RequestStream,
        next: Next,
    ) -> Result<ResponseStream> {
        // Forward the request
        let mut response_stream = next(request).await?;

        // Create a new stream that intercepts cost information
        let state_backend = self.state_backend.clone();
        let ctx_clone = ctx.clone();

        let intercepted_stream = async_stream::stream! {
            let mut total_cost: Option<ActualCost> = None;
            let mut model = String::new();
            let mut provider = String::new();

            while let Some(chunk_result) = response_stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        // Intercept metadata for model/provider
                        // Extract metadata
                        if let ResponseChunk::Metadata(ref metadata) = chunk {
                            if let Some(m) = metadata.get("model").and_then(|v| v.as_str()) {
                                model = m.to_string();
                            }
                            if let Some(p) = metadata.get("provider").and_then(|v| v.as_str()) {
                                provider = p.to_string();
                            }
                        }

                        // Intercept final cost on Stop
                        if let ResponseChunk::Stop { cost: Some(ref c), .. } = chunk {
                            total_cost = Some(c.clone());
                        }

                        yield Ok(chunk);
                    }
                    Err(e) => {
                        yield Err(e);
                    }
                }
            }

            // Record usage if we have cost information
            if let Some(cost) = total_cost {
                let usage = UsageRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    org_id: ctx_clone.identity.context.org_id.clone().unwrap_or_default(),
                    user_id: ctx_clone.identity.context.user_id.clone().unwrap_or_default(),
                    api_key_hash: ctx_clone.identity.context.api_key_hash.clone().unwrap_or_default(),
                    request_id: ctx_clone.trace_id.clone().unwrap_or_default(),
                    provider_id: provider,
                    model_id: model,
                    input_tokens: cost.input_tokens as u64,
                    output_tokens: cost.output_tokens as u64,
                    total_tokens: (cost.input_tokens + cost.output_tokens) as u64,
                    cost: cost.total_cost_usd.to_string().parse().unwrap_or(0.0),
                    timestamp: chrono::Utc::now(),
                    metadata: std::collections::HashMap::new(),
                };

                if let Err(e) = state_backend.record_usage(&usage).await {
                    #[cfg(feature = "tracing")]
                    {
                        error!("Failed to record usage: {}", e);
                    }
                    let _ = e; // Suppress unused warning when tracing is disabled
                }
            }
        };

        Ok(Box::pin(intercepted_stream))
    }
}
