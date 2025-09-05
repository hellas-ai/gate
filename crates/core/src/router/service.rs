//! Helper service for routing from JSON requests

use crate::Result;
use crate::router::middleware::ResponseStream as MwResponseStream;
use crate::router::protocols;
use crate::router::routing::Router;
use crate::router::sink::RequestContext;
use crate::router::types::Protocol;
use crate::router::types::{RequestDescriptor, RequestStream};
use serde_json::Value as JsonValue;

/// Build a RequestDescriptor from a single JSON request body for a known protocol
pub fn descriptor_from_json_with_protocol(
    json: &JsonValue,
    protocol: Protocol,
) -> Result<RequestDescriptor> {
    let model = protocols::extract_model(json).unwrap_or_else(|| "unknown".to_string());
    let capabilities = protocols::extract_capabilities(json, protocol);

    // Best-effort context length hint: rough token estimate by chars/4
    let context_length_hint = Some(json.to_string().len() / 4);

    Ok(RequestDescriptor {
        model,
        protocol,
        capabilities,
        context_length_hint,
    })
}

/// Build a one-off RequestStream from a single JSON payload
pub fn one_shot_stream(protocol: Protocol, json: JsonValue) -> RequestStream {
    RequestStream::new(
        protocol,
        Box::pin(futures::stream::once(async move { Ok(json) })),
    )
}

/// Route and execute for a single JSON request with a known protocol
pub async fn route_and_execute_json_with_protocol(
    router: &Router,
    ctx: &RequestContext,
    protocol: Protocol,
    json: JsonValue,
) -> Result<MwResponseStream> {
    let desc = descriptor_from_json_with_protocol(&json, protocol)?;
    let plan = router.route(ctx, &desc).await?;
    let stream = one_shot_stream(protocol, json);
    router.execute(plan, stream).await
}
