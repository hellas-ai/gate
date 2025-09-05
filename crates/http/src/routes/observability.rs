//! Observability endpoints for metrics and health checks

use crate::types::HealthCheckResponse;
use axum::{
    Json, Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
#[cfg(all(feature = "otlp", not(target_arch = "wasm32")))]
use gate_core::tracing::prometheus::prometheus_format;
use tracing::instrument;

/// Health check endpoint
#[instrument(name = "health_check")]
pub async fn health_handler() -> Response {
    // TODO: Add more sophisticated health checks (database, upstream connectivity, etc.)
    let health_status = HealthCheckResponse {
        status: "healthy".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        service: "gate-daemon".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    (StatusCode::OK, Json(health_status)).into_response()
}

/// Prometheus metrics endpoint
#[cfg(feature = "otlp")]
pub async fn metrics_handler() -> Response {
    let metrics = prometheus_format();

    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        metrics,
    )
        .into_response()
}

/// Prometheus metrics endpoint (stub when otlp is disabled)
#[cfg(not(feature = "otlp"))]
pub async fn metrics_handler() -> Response {
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        "# Metrics collection disabled (compile with 'otlp' feature)\n",
    )
        .into_response()
}

/// Create observability router
pub fn router<T>() -> Router<T>
where
    T: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
}
