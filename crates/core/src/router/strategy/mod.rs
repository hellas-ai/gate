//! Routing strategy implementations

mod best_of_n;
mod cost;
mod latency;
mod provider_affinity;
mod weighted;

pub use best_of_n::{BestOfNStrategy, SelectionMethod};
pub use cost::CostOptimizedStrategy;
pub use latency::LatencyOptimizedStrategy;
pub use provider_affinity::ProviderAffinityStrategy;
pub use weighted::WeightedStrategy;

use super::protocols::ProtocolConversion;
use super::sink::{RequestContext, Sink, SinkDescription};
use super::types::{RequestDescriptor, SinkHealth};
use crate::Result;
use async_trait::async_trait;
use rust_decimal::Decimal;
use std::sync::Arc;
use std::time::Duration;

/// Candidate sink for routing
#[derive(Clone)]
pub struct SinkCandidate {
    pub sink: Arc<dyn Sink>,
    pub description: SinkDescription,
    pub health: SinkHealth,
    pub needs_conversion: Option<ProtocolConversion>,
}

/// Scored route after strategy evaluation
pub struct ScoredRoute {
    pub sink_id: String,
    pub score: f64,
    pub estimated_cost: Option<Decimal>,
    pub estimated_latency: Option<Duration>,
    pub conversion_needed: Option<ProtocolConversion>,
    pub rationale: String,
}

/// Trait for routing strategies
#[async_trait]
pub trait RoutingStrategy: Send + Sync {
    /// Evaluate candidates and return scored routes
    async fn evaluate(
        &self,
        ctx: &RequestContext,
        request: &RequestDescriptor,
        candidates: Vec<SinkCandidate>,
    ) -> Result<Vec<ScoredRoute>>;
}

/// Simple strategy that returns candidates in order
pub struct SimpleStrategy;

impl SimpleStrategy {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RoutingStrategy for SimpleStrategy {
    async fn evaluate(
        &self,
        _ctx: &RequestContext,
        _request: &RequestDescriptor,
        candidates: Vec<SinkCandidate>,
    ) -> Result<Vec<ScoredRoute>> {
        Ok(candidates
            .into_iter()
            .enumerate()
            .map(|(i, candidate)| ScoredRoute {
                sink_id: candidate.description.id,
                score: 1.0 / (i as f64 + 1.0), // Decreasing scores
                estimated_cost: None,
                estimated_latency: None,
                conversion_needed: candidate.needs_conversion,
                rationale: "Simple ordered routing".to_string(),
            })
            .collect())
    }
}

impl Default for SimpleStrategy {
    fn default() -> Self {
        Self::new()
    }
}

/// Composite strategy that combines multiple strategies
pub struct CompositeStrategy {
    strategies: Vec<(Box<dyn RoutingStrategy>, f64)>, // Strategy and weight
}

impl CompositeStrategy {
    pub fn new(strategies: Vec<(Box<dyn RoutingStrategy>, f64)>) -> Self {
        Self { strategies }
    }
}

#[async_trait]
impl RoutingStrategy for CompositeStrategy {
    async fn evaluate(
        &self,
        ctx: &RequestContext,
        request: &RequestDescriptor,
        candidates: Vec<SinkCandidate>,
    ) -> Result<Vec<ScoredRoute>> {
        if self.strategies.is_empty() {
            return Ok(vec![]);
        }

        // Collect scores from all strategies
        let mut all_scores: std::collections::HashMap<String, Vec<(f64, f64)>> =
            std::collections::HashMap::new();
        let mut final_routes: std::collections::HashMap<String, ScoredRoute> =
            std::collections::HashMap::new();

        for (strategy, weight) in &self.strategies {
            let scored = strategy.evaluate(ctx, request, candidates.clone()).await?;
            for route in scored {
                all_scores
                    .entry(route.sink_id.clone())
                    .or_default()
                    .push((route.score, *weight));

                // Store the route details (will be overwritten by last strategy)
                final_routes.insert(route.sink_id.clone(), route);
            }
        }

        // Calculate weighted average scores
        let mut result = Vec::new();
        for (sink_id, scores) in all_scores {
            let total_weight: f64 = scores.iter().map(|(_, w)| w).sum();
            let weighted_sum: f64 = scores.iter().map(|(s, w)| s * w).sum();
            let final_score = if total_weight > 0.0 {
                weighted_sum / total_weight
            } else {
                0.0
            };

            if let Some(mut route) = final_routes.remove(&sink_id) {
                route.score = final_score;
                route.rationale =
                    format!("Composite score from {} strategies", self.strategies.len());
                result.push(route);
            }
        }

        // Sort by score (highest first)
        result.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(result)
    }
}
