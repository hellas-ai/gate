use super::{RoutingStrategy, ScoredRoute, SinkCandidate};
use crate::Result;
use crate::router::sink::RequestContext;
use crate::router::types::RequestDescriptor;
use async_trait::async_trait;

pub struct ProviderAffinityStrategy;

impl Default for ProviderAffinityStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAffinityStrategy {
    pub fn new() -> Self {
        Self
    }

    fn provider_prefix_for_model(model: &str) -> Option<&'static str> {
        let m = model.to_lowercase();
        if m.starts_with("claude") {
            Some("provider://anthropic")
        } else if m.starts_with("gpt-") || m.starts_with("o1-") || m.starts_with("o3-") {
            Some("provider://openai")
        } else {
            None
        }
    }
}

#[async_trait]
impl RoutingStrategy for ProviderAffinityStrategy {
    async fn evaluate(
        &self,
        _ctx: &RequestContext,
        request: &RequestDescriptor,
        candidates: Vec<SinkCandidate>,
    ) -> Result<Vec<ScoredRoute>> {
        let hint = Self::provider_prefix_for_model(&request.model);

        let mut routes = Vec::with_capacity(candidates.len());
        for (i, c) in candidates.into_iter().enumerate() {
            let score = if let Some(prefix) = hint {
                if c.description.id.starts_with(prefix) {
                    1.0
                } else if c.description.id.starts_with("self://") {
                    0.2
                } else {
                    0.4
                }
            } else {
                // No hint; keep stable order but slightly decreasing score
                1.0 / (i as f64 + 1.0)
            };

            routes.push(ScoredRoute {
                sink_id: c.description.id.clone(),
                score,
                estimated_cost: None,
                estimated_latency: None,
                conversion_needed: c.needs_conversion,
                rationale: "Provider affinity".into(),
            });
        }

        // Sort by score (highest first)
        routes.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(routes)
    }
}
