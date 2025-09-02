//! Best-of-N sampling strategy

use super::{RoutingStrategy, ScoredRoute, SinkCandidate};
use crate::Result;
use crate::router::sink::RequestContext;
use crate::router::types::RequestDescriptor;
use async_trait::async_trait;

/// Selection method for best-of-N
#[derive(Debug, Clone)]
pub enum SelectionMethod {
    /// Return the first complete response
    FirstComplete,
    /// Return the most common response (majority vote)
    MajorityVote,
    /// Return the response with highest confidence
    HighestConfidence,
    /// Use a judge model to select the best
    JudgeModel(String),
}

/// Best-of-N sampling strategy
pub struct BestOfNStrategy {
    n: usize,
    selection: SelectionMethod,
}

impl BestOfNStrategy {
    /// Create a new best-of-N strategy
    pub fn new(n: usize, selection: SelectionMethod) -> Self {
        Self {
            n: n.max(1),
            selection,
        }
    }

    /// Create a first-complete strategy
    pub fn first_complete(n: usize) -> Self {
        Self::new(n, SelectionMethod::FirstComplete)
    }

    /// Create a majority vote strategy
    pub fn majority_vote(n: usize) -> Self {
        Self::new(n, SelectionMethod::MajorityVote)
    }

    /// Create a judge-based strategy
    pub fn with_judge(n: usize, judge_model: String) -> Self {
        Self::new(n, SelectionMethod::JudgeModel(judge_model))
    }
}

#[async_trait]
impl RoutingStrategy for BestOfNStrategy {
    async fn evaluate(
        &self,
        _ctx: &RequestContext,
        _request: &RequestDescriptor,
        mut candidates: Vec<SinkCandidate>,
    ) -> Result<Vec<ScoredRoute>> {
        // For best-of-N, we want to select N candidates that will all be executed
        // Score them equally high so they're all selected

        // Filter to healthy candidates only
        candidates.retain(|c| c.health.healthy);

        if candidates.is_empty() {
            return Ok(vec![]);
        }

        // Take up to N candidates
        let n_candidates = candidates.len().min(self.n);
        let selected = candidates.into_iter().take(n_candidates);

        let mut scored = Vec::new();
        for (i, candidate) in selected.enumerate() {
            // Give all selected candidates the same high score
            // They will all be executed in parallel
            let score = 1.0;

            let rationale = match &self.selection {
                SelectionMethod::FirstComplete => {
                    format!("Best-of-{}: candidate {} (first-complete)", self.n, i + 1)
                }
                SelectionMethod::MajorityVote => {
                    format!("Best-of-{}: candidate {} (majority-vote)", self.n, i + 1)
                }
                SelectionMethod::HighestConfidence => {
                    format!(
                        "Best-of-{}: candidate {} (highest-confidence)",
                        self.n,
                        i + 1
                    )
                }
                SelectionMethod::JudgeModel(model) => {
                    format!("Best-of-{}: candidate {} (judge: {})", self.n, i + 1, model)
                }
            };

            scored.push(ScoredRoute {
                sink_id: candidate.description.id.clone(),
                score,
                estimated_cost: None,
                estimated_latency: candidate
                    .health
                    .latency_ms
                    .map(std::time::Duration::from_millis),
                conversion_needed: candidate.needs_conversion.clone(),
                rationale,
            });
        }

        Ok(scored)
    }
}
