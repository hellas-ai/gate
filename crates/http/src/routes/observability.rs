//! Observability endpoints for metrics and health checks

use axum::{
    Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
#[cfg(all(feature = "otlp", not(target_arch = "wasm32")))]
use gate_core::tracing::prometheus::prometheus_format;

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
    Router::new().route("/metrics", get(metrics_handler))
}
