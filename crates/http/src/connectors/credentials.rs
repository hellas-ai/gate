use gate_core::router::connector::RequestContext;
use gate_core::router::credentials::{CredentialHeader, CredentialResolver};
use http::header::AUTHORIZATION;
use http::{HeaderName, HeaderValue};

use super::http_connector::Provider;

/// Resolve using a static API key configured for the provider
pub struct StaticKeyResolver {
    provider: Provider,
    api_key: String,
}

impl StaticKeyResolver {
    pub fn new(provider: Provider, api_key: String) -> Self {
        Self { provider, api_key }
    }
}

#[async_trait::async_trait]
impl CredentialResolver for StaticKeyResolver {
    async fn resolve(
        &self,
        _ctx: &RequestContext,
        _provider_hint: &str,
    ) -> Option<CredentialHeader> {
        match self.provider {
            Provider::Anthropic => {
                // Accept both Bearer and x-api-key styles based on key prefix
                if self.api_key.starts_with("sk-ant-oat01-") {
                    HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                        .ok()
                        .map(|v| (AUTHORIZATION, v))
                } else {
                    let name = HeaderName::from_static("x-api-key");
                    HeaderValue::from_str(&self.api_key).ok().map(|v| (name, v))
                }
            }
            Provider::OpenAI | Provider::OpenAICodex | Provider::Custom => {
                HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                    .ok()
                    .map(|v| (AUTHORIZATION, v))
            }
        }
    }
}

/// Resolve by passing through client-supplied headers (Authorization/x-api-key)
pub struct HeaderPassthroughResolver {
    provider: Provider,
}

impl HeaderPassthroughResolver {
    pub fn new(provider: Provider) -> Self {
        Self { provider }
    }
}

#[async_trait::async_trait]
impl CredentialResolver for HeaderPassthroughResolver {
    async fn resolve(
        &self,
        ctx: &RequestContext,
        _provider_hint: &str,
    ) -> Option<CredentialHeader> {
        match self.provider {
            Provider::Anthropic => {
                let x_api = HeaderName::from_static("x-api-key");
                if let Some(val) = ctx.headers.get(&x_api)
                    && let Ok(key) = val.to_str()
                    && !key.is_empty()
                    && let Ok(hv) = HeaderValue::from_str(key)
                {
                    return Some((x_api, hv));
                }
                if let Some(val) = ctx.headers.get(AUTHORIZATION)
                    && let Ok(auth) = val.to_str()
                    && auth.starts_with("Bearer sk-ant-")
                    && let Ok(hv) = HeaderValue::from_str(auth)
                {
                    return Some((AUTHORIZATION, hv));
                }
                None
            }
            Provider::OpenAI | Provider::OpenAICodex | Provider::Custom => {
                if let Some(val) = ctx.headers.get(AUTHORIZATION)
                    && let Ok(auth) = val.to_str()
                    && auth.starts_with("Bearer ")
                    && let Ok(hv) = HeaderValue::from_str(auth)
                {
                    return Some((AUTHORIZATION, hv));
                }
                let x_api = HeaderName::from_static("x-api-key");
                if let Some(val) = ctx.headers.get(&x_api)
                    && let Ok(key) = val.to_str()
                    && !key.is_empty()
                    && let Ok(hv) = HeaderValue::from_str(&format!("Bearer {key}"))
                {
                    return Some((AUTHORIZATION, hv));
                }
                None
            }
        }
    }
}
