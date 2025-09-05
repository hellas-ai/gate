//! Latency-optimized routing strategy

use super::{RoutingStrategy, ScoredRoute, SinkCandidate};
use crate::Result;
use crate::router::sink::RequestContext;
use crate::router::types::RequestDescriptor;
use async_trait::async_trait;
use std::time::Duration;

/// Latency-optimized routing strategy
pub struct LatencyOptimizedStrategy {
    max_latency: Option<Duration>,
    percentile: f64, // e.g., 0.95 for p95
}

impl LatencyOptimizedStrategy {
    /// Create a new latency-optimized strategy
    pub fn new() -> Self {
        Self {
            max_latency: None,
            percentile: 0.95,
        }
    }

    /// Create with a maximum latency threshold
    pub fn with_max_latency(max_latency: Duration) -> Self {
        Self {
            max_latency: Some(max_latency),
            percentile: 0.95,
        }
    }

    /// Set the percentile to optimize for (e.g., 0.99 for p99)
    pub fn with_percentile(mut self, percentile: f64) -> Self {
        self.percentile = percentile.clamp(0.0, 1.0);
        self
    }
}

impl Default for LatencyOptimizedStrategy {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RoutingStrategy for LatencyOptimizedStrategy {
    async fn evaluate(
        &self,
        _ctx: &RequestContext,
        _request: &RequestDescriptor,
        candidates: Vec<SinkCandidate>,
    ) -> Result<Vec<ScoredRoute>> {
        let mut scored = Vec::new();

        // Find the minimum latency for normalization
        let min_latency = candidates
            .iter()
            .filter_map(|c| c.health.latency_ms)
            .min()
            .unwrap_or(0);

        for candidate in candidates {
            let latency_ms = candidate.health.latency_ms.unwrap_or(1000); // Default to 1s if unknown
            let latency = Duration::from_millis(latency_ms);

            // Skip if over max latency threshold
            if let Some(max) = self.max_latency
                && latency > max
            {
                continue;
            }

            // Calculate latency score (lower latency = higher score)
            let latency_score = if latency_ms > 0 {
                // Normalize to 0-1 range, with exponential decay
                let normalized = (min_latency as f64) / (latency_ms as f64);
                normalized.powf(2.0) // Square to heavily favor lower latencies
            } else {
                1.0
            };

            // Factor in health
            let health_factor = if candidate.health.healthy {
                1.0 - (candidate.health.error_rate as f64 * 0.5) // Error rate impacts score
            } else {
                0.1 // Severely penalize unhealthy sinks
            };

            // Consider protocol conversion overhead
            let conversion_penalty = if candidate.needs_conversion.is_some() {
                0.95 // 5% penalty for needing conversion
            } else {
                1.0
            };

            let final_score = latency_score * health_factor * conversion_penalty;

            scored.push(ScoredRoute {
                sink_id: candidate.description.id.clone(),
                score: final_score,
                estimated_cost: None,
                estimated_latency: Some(latency),
                conversion_needed: candidate.needs_conversion.clone(),
                rationale: format!(
                    "Latency-optimized: {}ms (p{:.0}), score={:.3}",
                    latency_ms,
                    self.percentile * 100.0,
                    final_score
                ),
            });
        }

        // Sort by score (highest first)
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(scored)
    }
}
