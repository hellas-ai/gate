//! Rate limiting middleware

use super::{Middleware, Next, RequestStream, ResponseStream};
use crate::Result;
use crate::router::connector::RequestContext;
use crate::router::types::QuotaBehavior;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};

/// Trait for pluggable rate limit queue backends
#[async_trait]
pub trait RateLimitQueue: Send + Sync {
    async fn enqueue(&self, key: &str, timeout: Duration) -> Result<()>;
    async fn release(&self, key: &str);
}

/// Rate limit configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub tokens_per_minute: Option<u32>,
    pub behavior: QuotaBehavior,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 60,
            tokens_per_minute: Some(100_000),
            behavior: QuotaBehavior::Reject,
        }
    }
}

/// Rate limiter state for a single key
#[derive(Debug)]
struct RateLimitState {
    request_count: u32,
    token_count: u32,
    window_start: Instant,
}

impl RateLimitState {
    fn new() -> Self {
        Self {
            request_count: 0,
            token_count: 0,
            window_start: Instant::now(),
        }
    }

    fn reset_if_needed(&mut self, window_duration: Duration) {
        if self.window_start.elapsed() >= window_duration {
            self.request_count = 0;
            self.token_count = 0;
            self.window_start = Instant::now();
        }
    }
}

/// Rate limiting middleware
pub struct RateLimitMiddleware {
    config: RateLimitConfig,
    states: Arc<RwLock<HashMap<String, RateLimitState>>>,
    queue: Option<Arc<dyn RateLimitQueue>>, // optional external queue implementation
}

impl RateLimitMiddleware {
    /// Create a new rate limit middleware
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            states: Arc::new(RwLock::new(HashMap::new())),
            queue: None,
        }
    }

    /// Set a pluggable queue implementation
    pub fn with_queue(mut self, queue: Arc<dyn RateLimitQueue>) -> Self {
        self.queue = Some(queue);
        self
    }

    /// Get the rate limit key from the context
    fn get_rate_limit_key(ctx: &RequestContext) -> String {
        // Use user ID if available, otherwise use org ID, otherwise use a default
        ctx.identity
            .context
            .user_id
            .clone()
            .or_else(|| ctx.identity.context.org_id.clone())
            .unwrap_or_else(|| "default".to_string())
    }

    /// Check if the request is within rate limits
    async fn check_rate_limit(&self, key: &str) -> Result<bool> {
        let mut states = self.states.write().await;
        let state = states
            .entry(key.to_string())
            .or_insert_with(RateLimitState::new);

        // Reset window if needed
        state.reset_if_needed(Duration::from_secs(60));

        // Check request limit
        if state.request_count >= self.config.requests_per_minute {
            return match self.config.behavior {
                QuotaBehavior::Reject => Err(crate::Error::QuotaExceeded(format!(
                    "Request rate limit exceeded: {} requests per minute",
                    self.config.requests_per_minute
                ))),
                QuotaBehavior::WarnOnly => {
                    #[cfg(feature = "tracing")]
                    {
                        warn!(
                            "Request rate limit exceeded for {}: {} requests",
                            key, state.request_count
                        );
                    }
                    Ok(true)
                }
                QuotaBehavior::TrackOverage => {
                    #[cfg(feature = "tracing")]
                    {
                        info!(
                            "Tracking overage for {}: {} requests over limit",
                            key,
                            state.request_count - self.config.requests_per_minute
                        );
                    }
                    Ok(true)
                }
            };
        }

        // Increment request count
        state.request_count += 1;
        Ok(true)
    }
}

#[async_trait]
impl Middleware for RateLimitMiddleware {
    async fn process(
        &self,
        ctx: &mut RequestContext,
        request: RequestStream,
        next: Next,
    ) -> Result<ResponseStream> {
        let key = Self::get_rate_limit_key(ctx);

        // Check rate limit
        // If over limits and behavior is Queue, optionally enqueue
        match self.check_rate_limit(&key).await {
            Ok(_) => {}
            Err(e) => {
                if let QuotaBehavior::Reject = self.config.behavior {
                    return Err(e);
                }
                if let QuotaBehavior::TrackOverage | QuotaBehavior::WarnOnly = self.config.behavior
                {
                    // pass through
                }
            }
        }

        // Process the request
        let response = next(request).await?;

        // TODO: Token usage tracking could be added by inspecting the response stream

        Ok(response)
    }
}
