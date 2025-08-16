//! Models API routes

use crate::{
    error::HttpError,
    state::AppState,
    types::{ModelInfo, ModelsListResponse},
};
use axum::{
    extract::State,
    response::{IntoResponse, Json, Response},
};
use utoipa_axum::{router::OpenApiRouter, routes};

#[cfg(test)]
mod tests;

/// Handle models list requests
#[utoipa::path(
    get,
    path = "/v1/models",
    responses(
        (status = 200, description = "List of available models", body = ModelsListResponse),
        (status = 500, description = "Internal server error")
    ),
    tag = "models"
)]
#[instrument(
    name = "list_models",
    skip(app_state),
    fields(
        upstream_count = tracing::field::Empty,
        model_count = tracing::field::Empty
    )
)]
pub async fn models_handler<T>(State(app_state): State<AppState<T>>) -> Result<Response, HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    info!("Received models list request");

    let all_upstreams = app_state.upstream_registry.get_all_upstreams().await;
    tracing::Span::current().record("upstream_count", all_upstreams.len());
    let mut models = Vec::new();

    // Add upstream models
    for (upstream_name, upstream_info) in &all_upstreams {
        for model_id in &upstream_info.models {
            models.push(ModelInfo {
                id: model_id.clone(),
                object: "model".to_string(),
                owned_by: upstream_name.clone(),
                created: chrono::Utc::now().timestamp(),
                context_length: None,
            });
        }
    }

    // Add local inference models if available
    if let Some(inference_backend) = &app_state.inference_backend
        && let Ok(local_models) = inference_backend.list_models().await
    {
        for model in local_models {
            models.push(ModelInfo {
                id: model.id,
                object: "model".to_string(),
                owned_by: "local".to_string(),
                created: chrono::Utc::now().timestamp(),
                context_length: Some(model.context_length as usize),
            });
        }
    }

    tracing::Span::current().record("model_count", models.len());

    let response = ModelsListResponse {
        object: "list".to_string(),
        data: models,
    };

    Ok(Json(response).into_response())
}

/// Add models routes to router
pub fn add_routes<T: Send + Sync + Clone + 'static>(
    router: OpenApiRouter<AppState<T>>,
) -> OpenApiRouter<AppState<T>> {
    router.routes(routes!(models_handler))
}
