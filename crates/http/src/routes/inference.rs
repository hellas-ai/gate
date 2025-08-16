//! Inference API routes for LLM providers

use crate::{dispatcher::Dispatcher, error::HttpError, state::AppState, types::*};
use axum::{
    extract::{Json, State},
    http::HeaderMap,
    response::Response,
};
use gate_core::tracing::prelude::*;
use serde_json::Value as JsonValue;
use utoipa_axum::{router::OpenApiRouter, routes};

/// Handle Anthropic messages requests
#[utoipa::path(
    post,
    path = "/v1/messages",
    request_body = AnthropicMessagesRequest,
    responses(
        (status = 200, description = "Successful response", body = JsonValue),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error")
    ),
    tag = "inference"
)]
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
    headers: HeaderMap,
    axum::Extension(correlation_id): axum::Extension<CorrelationId>,
    Json(request): Json<AnthropicMessagesRequest>,
) -> Result<Response, HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    let dispatcher = if let Some(inference_backend) = &app_state.inference_backend {
        Dispatcher::with_inference_backend(
            app_state.upstream_registry.clone(),
            inference_backend.clone(),
        )
    } else {
        Dispatcher::new(app_state.upstream_registry.clone())
    };

    dispatcher
        .messages(
            &request.model,
            serde_json::to_value(&request).map_err(|e| {
                HttpError::InternalServerError(format!("Failed to serialize request: {e}"))
            })?,
            headers,
            correlation_id,
        )
        .await
}

/// Handle OpenAI chat completions requests
#[utoipa::path(
    post,
    path = "/v1/chat/completions",
    request_body = OpenAIChatCompletionRequest,
    responses(
        (status = 200, description = "Successful completion", body = JsonValue),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error")
    ),
    tag = "inference"
)]
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
    headers: HeaderMap,
    axum::Extension(correlation_id): axum::Extension<CorrelationId>,
    Json(request): Json<OpenAIChatCompletionRequest>,
) -> Result<Response, HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    let dispatcher = if let Some(inference_backend) = &app_state.inference_backend {
        Dispatcher::with_inference_backend(
            app_state.upstream_registry.clone(),
            inference_backend.clone(),
        )
    } else {
        Dispatcher::new(app_state.upstream_registry.clone())
    };

    dispatcher
        .chat_completions(
            &request.model,
            serde_json::to_value(&request).map_err(|e| {
                HttpError::InternalServerError(format!("Failed to serialize request: {e}"))
            })?,
            headers,
            correlation_id,
        )
        .await
}

/// Handle OpenAI responses requests
#[utoipa::path(
    post,
    path = "/v1/responses",
    request_body = OpenAICompletionRequest,
    responses(
        (status = 200, description = "Successful response", body = JsonValue),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error")
    ),
    tag = "inference"
)]
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
    headers: HeaderMap,
    axum::Extension(correlation_id): axum::Extension<CorrelationId>,
    Json(request): Json<OpenAICompletionRequest>,
) -> Result<Response, HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    let dispatcher = if let Some(inference_backend) = &app_state.inference_backend {
        Dispatcher::with_inference_backend(
            app_state.upstream_registry.clone(),
            inference_backend.clone(),
        )
    } else {
        Dispatcher::new(app_state.upstream_registry.clone())
    };

    dispatcher
        .responses(
            &request.model,
            serde_json::to_value(&request).map_err(|e| {
                HttpError::InternalServerError(format!("Failed to serialize request: {e}"))
            })?,
            headers,
            correlation_id,
        )
        .await
}

/// Add inference routes to router
pub fn add_routes<T: Send + Sync + Clone + 'static>(
    router: OpenApiRouter<AppState<T>>,
) -> OpenApiRouter<AppState<T>> {
    router
        .routes(routes!(chat_completions_handler))
        .routes(routes!(responses_handler))
        .routes(routes!(messages_handler))
}
