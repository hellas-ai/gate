//! Correlation ID middleware for request tracing
//!
//! This middleware extracts or generates correlation IDs for all requests,
//! supporting both W3C TraceContext headers and legacy x-correlation-id.

use axum::{
    extract::Request,
    http::{HeaderMap, HeaderName, HeaderValue},
    middleware::Next,
    response::IntoResponse,
};
use gate_core::tracing::{
    prelude::*,
    trace_context::{extract_trace_context, inject_trace_context},
};
use tracing::Instrument;

/// Header name for legacy correlation ID
pub const CORRELATION_ID_HEADER: &str = "x-correlation-id";
/// W3C trace context header
pub const TRACEPARENT_HEADER: &str = "traceparent";
/// W3C trace state header
pub const TRACESTATE_HEADER: &str = "tracestate";

/// Extract or generate correlation ID from request headers
///
/// Priority:
/// 1. W3C traceparent header
/// 2. Legacy x-correlation-id header
/// 3. Generate new
pub fn extract_correlation_id(headers: &HeaderMap) -> CorrelationId {
    // Try to extract W3C trace context first
    if let Some(trace_context) = extract_trace_context(headers) {
        return CorrelationId::from_trace_context(trace_context);
    }

    // Fallback to legacy correlation ID
    headers
        .get(CORRELATION_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(CorrelationId::from_legacy)
        .unwrap_or_default()
}

/// Middleware to handle correlation IDs
/// Middleware to handle correlation IDs
#[allow(clippy::manual_async_fn)]
pub fn correlation_id_middleware(
    mut request: Request,
    next: Next,
) -> impl std::future::Future<Output = impl IntoResponse> + Send {
    async move {
        // Extract or generate correlation ID
        let correlation_id = extract_correlation_id(request.headers());

        // Add correlation ID to request extensions early so it's available to other middleware
        request.extensions_mut().insert(correlation_id.clone());

        // Set up the OpenTelemetry context and create span within it
        #[cfg(all(feature = "otlp", not(target_arch = "wasm32")))]
        {
            use opentelemetry::{Context as OtelContext, trace::TraceContextExt};

            // Create OpenTelemetry context with the remote span context
            let span_context = correlation_id.to_span_context();
            let _otel_context = OtelContext::current().with_remote_span_context(span_context);

            // Create a span for this request with trace context information
            let span = tracing::info_span!(
                "http_request",
                correlation_id = %correlation_id,
                method = %request.method(),
                path = %request.uri().path(),
                otel.name = "http_request",
                otel.kind = "SERVER",
                // Include trace and span IDs for OpenTelemetry
                trace_id = %correlation_id.trace_id(),
                span_id = %correlation_id.span_id(),
                // Include the traceparent header for propagation
                traceparent = %correlation_id.to_traceparent()
            );

            // Process request with the span attached
            // Note: When compiling with all features, we skip the context guard to avoid Send issues
            // The span itself still provides the trace context for OpenTelemetry
            let mut response = next.run(request).instrument(span).await;

            // Add both W3C and legacy headers to response
            let headers = response.headers_mut();

            // Always inject W3C headers
            if let Err(e) = inject_trace_context(correlation_id.trace_context(), headers) {
                tracing::warn!("Failed to inject trace context headers: {}", e);
            }

            // Also add legacy header for backward compatibility
            if let Ok(header_value) = HeaderValue::from_str(&correlation_id.to_string()) {
                headers.insert(HeaderName::from_static(CORRELATION_ID_HEADER), header_value);
            }

            response
        }

        // Non-OTLP native path
        #[cfg(all(not(feature = "otlp"), not(target_arch = "wasm32")))]
        {
            // Create a span for this request with trace context information
            let span = tracing::info_span!(
                "http_request",
                correlation_id = %correlation_id,
                method = %request.method(),
                path = %request.uri().path(),
                otel.name = "http_request",
                otel.kind = "SERVER",
                // Include trace and span IDs for OpenTelemetry
                trace_id = %correlation_id.trace_id(),
                span_id = %correlation_id.span_id(),
                // Include the traceparent header for propagation
                traceparent = %correlation_id.to_traceparent()
            );

            // Process request with the span attached
            let mut response = next.run(request).instrument(span).await;

            // Add both W3C and legacy headers to response
            let headers = response.headers_mut();

            // Always inject W3C headers
            if let Err(e) = inject_trace_context(correlation_id.trace_context(), headers) {
                tracing::warn!("Failed to inject trace context headers: {}", e);
            }

            // Also add legacy header for backward compatibility
            if let Ok(header_value) = HeaderValue::from_str(&correlation_id.to_string()) {
                headers.insert(HeaderName::from_static(CORRELATION_ID_HEADER), header_value);
            }

            response
        }

        // WASM path
        #[cfg(target_arch = "wasm32")]
        {
            // Create a span for this request with trace context information
            let span = tracing::info_span!(
                "http_request",
                correlation_id = %correlation_id,
                method = %request.method(),
                path = %request.uri().path(),
                otel.name = "http_request",
                otel.kind = "SERVER",
                // Include trace and span IDs for OpenTelemetry
                trace_id = %correlation_id.trace_id(),
                span_id = %correlation_id.span_id(),
                // Include the traceparent header for propagation
                traceparent = %correlation_id.to_traceparent()
            );

            // Process request with the span attached
            let mut response = next.run(request).instrument(span).await;

            // Add both W3C and legacy headers to response
            let headers = response.headers_mut();

            // Always inject W3C headers
            if let Err(e) = inject_trace_context(correlation_id.trace_context(), headers) {
                tracing::warn!("Failed to inject trace context headers: {}", e);
            }

            // Also add legacy header for backward compatibility
            if let Ok(header_value) = HeaderValue::from_str(&correlation_id.to_string()) {
                headers.insert(HeaderName::from_static(CORRELATION_ID_HEADER), header_value);
            }

            response
        }
    }
}

/// Extension trait for extracting correlation ID from request
pub trait CorrelationIdExt {
    /// Get the correlation ID from request extensions
    fn correlation_id(&self) -> Option<&CorrelationId>;
}

impl CorrelationIdExt for Request {
    fn correlation_id(&self) -> Option<&CorrelationId> {
        self.extensions().get::<CorrelationId>()
    }
}
