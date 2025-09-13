//! Models API routes

use crate::{
    error::HttpError,
    state::AppState,
    types::{ModelInfo, ModelsListResponse},
};
use axum::{
    Router,
    extract::State,
    response::{IntoResponse, Json, Response},
    routing::get,
};
use gate_core::router::prelude::Sink;
use tracing::{info, instrument};

/// Handle models list requests
#[instrument(
    name = "list_models",
    skip(app_state),
    fields(
        model_count = tracing::field::Empty
    )
)]
pub async fn models_handler<T>(State(app_state): State<AppState<T>>) -> Result<Response, HttpError>
where
    T: Clone + Send + Sync + 'static,
{
    info!("Received models list request");

    let mut models = Vec::new();

    // Get models from router if available
    if let Some(router) = &app_state.router {
        let desc = router.describe().await;
        // match desc.models {
        //     gate_core::router::types::ModelList::Static(list) => {
        //         for id in list {
        //             models.push(ModelInfo {
        //                 id,
        //                 object: "model".to_string(),
        //                 owned_by: "system".to_string(),
        //                 created: chrono::Utc::now().timestamp(),
        //                 context_length: desc.capabilities.max_context_length,
        //             });
        //         }
        //     }
        //     gate_core::router::types::ModelList::Dynamic => {
        //         // Dynamic models are resolved at runtime
        //         info!("Router has dynamic model list");
        //     }
        //     gate_core::router::types::ModelList::Infinite => {
        //         // Router accepts any model name
        //         info!("Router accepts any model");
        //     }
        // }
    }

    tracing::Span::current().record("model_count", models.len());

    let response = ModelsListResponse {
        object: "list".to_string(),
        data: models,
    };

    Ok(Json(response).into_response())
}

/// Create models router
pub fn router<T>() -> Router<AppState<T>>
where
    T: Send + Sync + Clone + 'static,
{
    Router::new().route("/v1/models", get(models_handler))
}
