//! Observability endpoints for metrics and health checks

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
#[cfg(all(feature = "otlp", not(target_arch = "wasm32")))]
use gate_core::tracing::prometheus::prometheus_format;
use serde_json::json;
use tracing::instrument;
use utoipa_axum::{router::OpenApiRouter, routes};

/// Health check endpoint
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is healthy", body = serde_json::Value),
        (status = 503, description = "Service is unhealthy", body = serde_json::Value)
    ),
    tag = "observability"
)]
#[instrument(name = "health_check")]
pub async fn health_handler() -> Response {
    // TODO: Add more sophisticated health checks (database, upstream connectivity, etc.)
    let health_status = json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "service": "gate-daemon",
        "version": env!("CARGO_PKG_VERSION"),
    });

    (StatusCode::OK, axum::Json(health_status)).into_response()
}

/// Prometheus metrics endpoint
#[cfg(feature = "otlp")]
#[utoipa::path(
    get,
    path = "/metrics",
    responses(
        (status = 200, description = "Prometheus metrics", content_type = "text/plain")
    ),
    tag = "observability"
)]
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
#[utoipa::path(
    get,
    path = "/metrics",
    responses(
        (status = 200, description = "Prometheus metrics", content_type = "text/plain")
    ),
    tag = "observability"
)]
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

/// Add observability routes to the router
pub fn add_routes<T>(router: OpenApiRouter<T>) -> OpenApiRouter<T>
where
    T: Clone + Send + Sync + 'static,
{
    router
        .routes(routes!(health_handler))
        .routes(routes!(metrics_handler))
}
