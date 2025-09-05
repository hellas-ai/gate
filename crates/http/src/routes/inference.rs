//! Inference API routes for LLM providers

use crate::{
    auth::extract_identity,
    error::HttpError,
    sinks::response_converter::{response_stream_to_axum, response_stream_to_json},
    state::AppState,
    types::*,
};
use http::header::HeaderName;
const X_TRACE_ID: HeaderName = HeaderName::from_static("x-trace-id");
use axum::{
    Router,
    extract::{Json, State},
    http::HeaderMap,
    response::Response,
    routing::post,
};
use gate_core::router::{
    service::route_and_execute_json_with_protocol, sink::RequestContext, types::Protocol,
};
use gate_core::tracing::prelude::*;

/// Handle Anthropic messages requests
#[instrument(
    name = "anthropic_messages",
    skip(app_state, headers),
    fields(
        model = %request.model,
        request_id = tracing::field::Empty
    )
)]
pub async fn messages_handler<T>(
    State(app_state): State<AppState<T>>,
    uri: axum::http::Uri,
    headers: HeaderMap,
    axum::Extension(correlation_id): axum::Extension<CorrelationId>,
    Json(request): Json<AnthropicMessagesRequest>,
) -> Result<Response, HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    let router = app_state
        .router
        .ok_or_else(|| HttpError::InternalServerError("Router not configured".to_string()))?;

    let ctx = RequestContext {
        identity: extract_identity(&headers),
        correlation_id,
        headers: headers.clone(),
        query: uri.query().map(|s| s.to_string()),
        trace_id: headers
            .get(X_TRACE_ID)
            .and_then(|v| v.to_str().ok())
            .map(String::from),
        metadata: Default::default(),
    };

    let request_json = serde_json::to_value(&request)
        .map_err(|e| HttpError::InternalServerError(format!("Failed to serialize request: {e}")))?;

    let stream = route_and_execute_json_with_protocol(
        router.as_ref(),
        &ctx,
        Protocol::Anthropic,
        request_json,
    )
    .await?;

    if request.stream {
        response_stream_to_axum(stream).await
    } else {
        response_stream_to_json(stream).await
    }
}

/// Handle OpenAI chat completions requests
#[instrument(
    name = "openai_chat_completions",
    skip(app_state, headers),
    fields(
        model = %request.model,
        stream = %request.stream
    )
)]
pub async fn chat_completions_handler<T>(
    State(app_state): State<AppState<T>>,
    uri: axum::http::Uri,
    headers: HeaderMap,
    axum::Extension(correlation_id): axum::Extension<CorrelationId>,
    Json(request): Json<OpenAIChatCompletionRequest>,
) -> Result<Response, HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    let router = app_state
        .router
        .ok_or_else(|| HttpError::InternalServerError("Router not configured".to_string()))?;

    let ctx = RequestContext {
        identity: extract_identity(&headers),
        correlation_id,
        headers: headers.clone(),
        query: uri.query().map(|s| s.to_string()),
        trace_id: headers
            .get(X_TRACE_ID)
            .and_then(|v| v.to_str().ok())
            .map(String::from),
        metadata: Default::default(),
    };

    let request_json = serde_json::to_value(&request)
        .map_err(|e| HttpError::InternalServerError(format!("Failed to serialize request: {e}")))?;

    let stream = route_and_execute_json_with_protocol(
        router.as_ref(),
        &ctx,
        Protocol::OpenAIChat,
        request_json,
    )
    .await?;

    if request.stream {
        response_stream_to_axum(stream).await
    } else {
        response_stream_to_json(stream).await
    }
}

/// Handle OpenAI responses requests
#[instrument(
    name = "openai_responses",
    skip(app_state, headers),
    fields(
        model = %request.model,
        stream = %request.stream
    )
)]
pub async fn responses_handler<T>(
    State(app_state): State<AppState<T>>,
    uri: axum::http::Uri,
    headers: HeaderMap,
    axum::Extension(correlation_id): axum::Extension<CorrelationId>,
    Json(request): Json<OpenAICompletionRequest>,
) -> Result<Response, HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    let router = app_state
        .router
        .ok_or_else(|| HttpError::InternalServerError("Router not configured".to_string()))?;

    let ctx = RequestContext {
        identity: extract_identity(&headers),
        correlation_id,
        headers: headers.clone(),
        query: uri.query().map(|s| s.to_string()),
        trace_id: headers
            .get(X_TRACE_ID)
            .and_then(|v| v.to_str().ok())
            .map(String::from),
        metadata: Default::default(),
    };

    let request_json = serde_json::to_value(&request)
        .map_err(|e| HttpError::InternalServerError(format!("Failed to serialize request: {e}")))?;

    let stream = route_and_execute_json_with_protocol(
        router.as_ref(),
        &ctx,
        Protocol::OpenAIResponses,
        request_json,
    )
    .await?;

    if request.stream {
        response_stream_to_axum(stream).await
    } else {
        response_stream_to_json(stream).await
    }
}

/// Create inference router
pub fn router<T>() -> Router<AppState<T>>
where
    T: Send + Sync + Clone + 'static,
{
    Router::new()
        .route("/v1/chat/completions", post(chat_completions_handler))
        .route("/v1/responses", post(responses_handler))
        .route("/v1/completions", post(completions_handler))
        .route("/v1/messages", post(messages_handler))
}

/// Handle OpenAI completions (legacy) requests
#[instrument(
    name = "openai_completions",
    skip(app_state, headers),
    fields(
        model = %request.model,
        stream = %request.stream
    )
)]
pub async fn completions_handler<T>(
    State(app_state): State<AppState<T>>,
    uri: axum::http::Uri,
    headers: HeaderMap,
    axum::Extension(correlation_id): axum::Extension<CorrelationId>,
    Json(request): Json<OpenAICompletionRequest>,
) -> Result<Response, HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    let router = app_state
        .router
        .ok_or_else(|| HttpError::InternalServerError("Router not configured".to_string()))?;

    let ctx = RequestContext {
        identity: extract_identity(&headers),
        correlation_id,
        headers: headers.clone(),
        query: uri.query().map(|s| s.to_string()),
        trace_id: headers
            .get(X_TRACE_ID)
            .and_then(|v| v.to_str().ok())
            .map(String::from),
        metadata: Default::default(),
    };

    let request_json = serde_json::to_value(&request)
        .map_err(|e| HttpError::InternalServerError(format!("Failed to serialize request: {e}")))?;

    let stream = route_and_execute_json_with_protocol(
        router.as_ref(),
        &ctx,
        Protocol::OpenAICompletions,
        request_json,
    )
    .await?;

    if request.stream {
        response_stream_to_axum(stream).await
    } else {
        response_stream_to_json(stream).await
    }
}
