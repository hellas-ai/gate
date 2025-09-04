use super::{Middleware, Next, RequestStream, ResponseStream};
use crate::Result;
use crate::router::sink::RequestContext;
use crate::router::types::{Protocol, ResponseChunk, StopReason};
use async_trait::async_trait;
use futures::StreamExt;
use http::header::AUTHORIZATION;
use std::sync::Arc;

/// Hook for registering providers on successful responses
#[async_trait]
pub trait KeyCaptureRegistrar: Send + Sync {
    async fn register_anthropic_key(&self, key: &str) -> Result<()>;
}

/// Middleware that captures Anthropic API keys on successful requests
pub struct KeyCaptureMiddleware<R: KeyCaptureRegistrar + 'static> {
    registrar: Arc<R>,
}

impl<R: KeyCaptureRegistrar + 'static> KeyCaptureMiddleware<R> {
    pub fn new(registrar: Arc<R>) -> Self {
        Self { registrar }
    }

    fn extract_anthropic_key(ctx: &RequestContext) -> Option<String> {
        if let Some(val) = ctx.headers.get("x-api-key")
            && let Ok(key) = val.to_str()
            && key.starts_with("sk-ant-")
        {
            return Some(key.to_string());
        }
        if let Some(val) = ctx.headers.get(AUTHORIZATION)
            && let Ok(auth) = val.to_str()
            && let Some(token) = auth.strip_prefix("Bearer ")
            && token.starts_with("sk-ant-")
        {
            return Some(token.to_string());
        }
        None
    }
}

#[async_trait]
impl<R: KeyCaptureRegistrar + 'static> Middleware for KeyCaptureMiddleware<R> {
    async fn process(
        &self,
        ctx: &mut RequestContext,
        request: RequestStream,
        next: Next,
    ) -> Result<ResponseStream> {
        // Only attempt capture for Anthropic protocol
        let protocol = request.protocol();
        let maybe_key = if protocol == Protocol::Anthropic {
            Self::extract_anthropic_key(ctx)
        } else {
            None
        };

        // Forward
        let mut response_stream = next(request).await?;

        if maybe_key.is_none() {
            return Ok(response_stream);
        }

        let key = maybe_key.unwrap();
        let registrar = self.registrar.clone();

        let intercepted = async_stream::stream! {
            let mut captured = false;
            while let Some(item) = response_stream.next().await {
                if let Ok(ResponseChunk::Stop { reason, error, .. }) = &item && !captured && matches!(reason, StopReason::Complete) && error.is_none() {
                        // Best-effort capture, ignore errors
                        let _ = registrar.register_anthropic_key(&key).await;
                        captured = true;
                }
                yield item;
            }
        };

        Ok(Box::pin(intercepted))
    }
}
