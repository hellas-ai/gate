//! Credential resolution abstraction

use http::{HeaderName, HeaderValue};

use super::connector::RequestContext;

/// Resolved credential represented as a header pair
pub type CredentialHeader = (HeaderName, HeaderValue);

/// Resolve credentials for a given provider/context
#[async_trait::async_trait]
pub trait CredentialResolver: Send + Sync {
    async fn resolve(&self, ctx: &RequestContext, provider_hint: &str) -> Option<CredentialHeader>;
}
