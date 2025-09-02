//! Cost-optimized routing strategy

use super::{RoutingStrategy, ScoredRoute, SinkCandidate};
use crate::Result;
use crate::router::sink::RequestContext;
use crate::router::types::RequestDescriptor;
use async_trait::async_trait;
use rust_decimal::Decimal;

/// Cost-optimized routing strategy
pub struct CostOptimizedStrategy {
    budget: Option<Decimal>,
    prefer_cached: bool,
}

impl CostOptimizedStrategy {
    /// Create a new cost-optimized strategy
    pub fn new() -> Self {
        Self {
            budget: None,
            prefer_cached: true,
        }
    }

    /// Create with a budget limit
    pub fn with_budget(budget: Decimal) -> Self {
        Self {
            budget: Some(budget),
            prefer_cached: true,
        }
    }

    /// Estimate cost for a request
    fn estimate_cost(
        &self,
        candidate: &SinkCandidate,
        request: &RequestDescriptor,
    ) -> Option<Decimal> {
        let cost_structure = candidate.description.cost_structure.as_ref()?;
        // Estimate tokens (rough): use context_length_hint and max_tokens if available
        let estimated_input_tokens = request.context_length_hint.unwrap_or(0) as u32;
        let max_tokens = request.capabilities.max_tokens.unwrap_or(512);
        let estimated_output_tokens = (max_tokens / 2).max(1);

        let input_cost =
            cost_structure.input_cost_per_token * Decimal::from(estimated_input_tokens);
        let output_cost =
            cost_structure.output_cost_per_token * Decimal::from(estimated_output_tokens);

        Some(input_cost + output_cost)
    }
}

impl Default for CostOptimizedStrategy {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RoutingStrategy for CostOptimizedStrategy {
    async fn evaluate(
        &self,
        _ctx: &RequestContext,
        request: &RequestDescriptor,
        candidates: Vec<SinkCandidate>,
    ) -> Result<Vec<ScoredRoute>> {
        let mut scored = Vec::new();

        for candidate in candidates {
            let estimated_cost = self.estimate_cost(&candidate, request);

            // Skip if over budget
            if let (Some(budget), Some(cost)) = (self.budget, estimated_cost) {
                if cost > budget {
                    continue;
                }
            }

            // Calculate score based on cost (lower cost = higher score)
            let cost_score = if let Some(cost) = estimated_cost {
                // Normalize cost to 0-1 range (assuming max cost of $1)
                let max_cost = Decimal::from(1);
                let normalized = (max_cost - cost.min(max_cost)) / max_cost;
                normalized.to_string().parse::<f64>().unwrap_or(0.5)
            } else {
                0.5 // Neutral score if cost unknown
            };

            // Boost score if sink supports caching and we prefer cached
            let cache_boost = if self.prefer_cached
                && candidate
                    .description
                    .cost_structure
                    .as_ref()
                    .and_then(|cs| cs.cached_input_cost_per_token)
                    .is_some()
            {
                0.1
            } else {
                0.0
            };

            // Penalize unhealthy sinks
            let health_penalty = if !candidate.health.healthy {
                0.5
            } else {
                candidate.health.error_rate as f64
            };

            let final_score = (cost_score + cache_boost) * (1.0 - health_penalty);

            scored.push(ScoredRoute {
                sink_id: candidate.description.id.clone(),
                score: final_score,
                estimated_cost,
                estimated_latency: candidate
                    .health
                    .latency_ms
                    .map(std::time::Duration::from_millis),
                conversion_needed: candidate.needs_conversion.clone(),
                rationale: format!(
                    "Cost-optimized: estimated_cost={:?}, score={:.3}",
                    estimated_cost, final_score
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
