//! Weighted routing strategy

use super::{RoutingStrategy, ScoredRoute, SinkCandidate};
use crate::Result;
use crate::router::sink::RequestContext;
use crate::router::types::RequestDescriptor;
use async_trait::async_trait;
use std::collections::HashMap;

/// Weighted routing strategy
pub struct WeightedStrategy {
    weights: HashMap<String, f64>,
    randomize: bool,
}

impl WeightedStrategy {
    /// Create a new weighted strategy
    pub fn new(weights: HashMap<String, f64>) -> Self {
        Self {
            weights,
            randomize: true,
        }
    }

    /// Create a deterministic weighted strategy (no randomization)
    pub fn deterministic(weights: HashMap<String, f64>) -> Self {
        Self {
            weights,
            randomize: false,
        }
    }
}

#[async_trait]
impl RoutingStrategy for WeightedStrategy {
    async fn evaluate(
        &self,
        _ctx: &RequestContext,
        _request: &RequestDescriptor,
        candidates: Vec<SinkCandidate>,
    ) -> Result<Vec<ScoredRoute>> {
        let mut scored = Vec::new();

        for candidate in candidates {
            let sink_id = &candidate.description.id;
            let base_weight = self.weights.get(sink_id).copied().unwrap_or(1.0);

            // Adjust weight based on health
            let health_factor = if candidate.health.healthy {
                1.0 - candidate.health.error_rate as f64
            } else {
                0.1 // Severely penalize unhealthy sinks
            };

            let score = if self.randomize {
                // Add some randomization for load balancing
                let random_factor = 0.8 + (rand::random::<f64>() * 0.4);
                base_weight * health_factor * random_factor
            } else {
                base_weight * health_factor
            };

            scored.push(ScoredRoute {
                sink_id: sink_id.clone(),
                score,
                estimated_cost: None,
                estimated_latency: candidate.health.latency_ms.map(Duration::from_millis),
                conversion_needed: candidate.needs_conversion.clone(),
                rationale: format!(
                    "Weighted routing: base_weight={base_weight:.2}, health_factor={health_factor:.2}"
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

// Helper for random number generation in WASM
#[cfg(target_arch = "wasm32")]
mod rand {
    pub fn random<T>() -> T
    where
        T: RandomValue,
    {
        T::random()
    }

    pub trait RandomValue {
        fn random() -> Self;
    }

    impl RandomValue for f64 {
        fn random() -> Self {
            // Simple pseudo-random for WASM
            let timestamp = js_sys::Date::now();
            ((timestamp as u64 * 1103515245 + 12345) % (1 << 31)) as f64 / (1 << 31) as f64
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod rand {
    pub fn random<T>() -> T
    where
        Standard: Distribution<T>,
    {
        use rand::Rng;
        rand::thread_rng().r#gen::<T>()
    }

    use rand::distributions::{Distribution, Standard};
}

use std::time::Duration;
