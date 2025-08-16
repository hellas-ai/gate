//! HTTP metrics middleware for request tracking
//!
//! This middleware collects metrics about HTTP requests including:
//! - Request count by method and status
//! - Request duration histogram
//! - Active request gauge

use axum::{extract::Request, middleware::Next, response::IntoResponse};
use gate_core::tracing::prelude::*;
use std::time::Instant;

/// Middleware to collect HTTP request metrics
pub async fn metrics_middleware(request: Request, next: Next) -> impl IntoResponse {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let start = Instant::now();

    // Create a span for metrics tracking
    let span = tracing::debug_span!(
        "http_metrics",
        method = %method,
        path = %path
    );

    // Increment active requests gauge
    gauge("http_requests_active").increment();

    // Process the request with the span attached
    let response = next.run(request).instrument(span).await;

    // Decrement active requests gauge
    gauge("http_requests_active").decrement();

    // Record request metrics
    let duration = start.elapsed();
    let status = response.status().as_u16();
    let status_class = format!("{}xx", status / 100);

    // Log request completion
    tracing::debug!(
        status = status,
        duration_ms = ?duration.as_millis(),
        "Request completed"
    );

    // Increment request counter by method and status class
    counter(&format!(
        "http_requests_total_{}_{}",
        method.to_lowercase(),
        status_class
    ))
    .increment();

    // Also track overall request count
    counter("http_requests_total").increment();

    // Record request duration by method
    histogram(&format!(
        "http_request_duration_seconds_{}",
        method.to_lowercase()
    ))
    .observe(duration.as_secs_f64());

    // Also record to overall histogram
    histogram("http_request_duration_seconds").observe(duration.as_secs_f64());

    response
}
