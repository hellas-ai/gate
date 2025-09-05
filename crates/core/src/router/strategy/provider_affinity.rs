use super::{RoutingStrategy, ScoredRoute, SinkCandidate};
use crate::Result;
use crate::router::sink::RequestContext;
use crate::router::types::{Protocol, RequestDescriptor};
use async_trait::async_trait;
use http::HeaderName;
use http::header::AUTHORIZATION;

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

    fn provider_prefix_for_protocol(proto: Protocol) -> Option<&'static str> {
        match proto {
            Protocol::Anthropic => Some("provider://anthropic"),
            Protocol::OpenAIChat
            | Protocol::OpenAIMessages
            | Protocol::OpenAICompletions
            | Protocol::OpenAIResponses => Some("provider://openai"),
            _ => None,
        }
    }

    fn provider_prefix_from_headers(headers: &http::HeaderMap) -> Option<&'static str> {
        // Anthropic signals
        let anth_version = HeaderName::from_static("anthropic-version");
        let x_api_key = HeaderName::from_static("x-api-key");
        if headers.contains_key(&anth_version) {
            return Some("provider://anthropic");
        }
        if let Some(val) = headers.get(&x_api_key).and_then(|v| v.to_str().ok())
            && val.starts_with("sk-ant-")
        {
            return Some("provider://anthropic");
        }
        if let Some(auth) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok())
            && let Some(token) = auth.strip_prefix("Bearer ")
            && token.starts_with("sk-ant-")
        {
            return Some("provider://anthropic");
        }

        // OpenAI signals
        let openai_beta = HeaderName::from_static("openai-beta");
        if headers.contains_key(&openai_beta) {
            return Some("provider://openai");
        }
        if let Some(auth) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok())
            && auth.starts_with("Bearer ")
        {
            return Some("provider://openai");
        }

        None
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
        // Derive intent from protocol, headers, then model prefix as weak hint
        let protocol_intent = Self::provider_prefix_for_protocol(request.protocol);
        let header_intent = Self::provider_prefix_from_headers(&_ctx.headers);
        let model_hint = Self::provider_prefix_for_model(&request.model);

        let mut routes = Vec::with_capacity(candidates.len());
        for (i, c) in candidates.into_iter().enumerate() {
            let mut score = 0.0;
            if let Some(p) = protocol_intent
                && c.description.id.starts_with(p)
            {
                score += 1.0;
            }
            if let Some(h) = header_intent
                && c.description.id.starts_with(h)
            {
                score += 0.6;
            }
            if let Some(m) = model_hint
                && c.description.id.starts_with(m)
            {
                score += 0.3;
            }
            // Prefer Codex backend for OpenAI Responses when using OAuth (non sk- token)
            if request.protocol == Protocol::OpenAIResponses
                && let Some(auth) = _ctx
                    .headers
                    .get(AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                && let Some(token) = auth.strip_prefix("Bearer ")
                && !token.starts_with("sk-")
                && c.description.id.starts_with("provider://openai/codex")
            {
                score += 0.5;
            }
            // Slight preference for configured/self sinks when otherwise tied
            if c.description.id.starts_with("self://") {
                score += 0.1;
            }
            // Ensure stable ordering when all signals are absent
            if score == 0.0 {
                score = 1.0 / (i as f64 + 1.0);
            }

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
