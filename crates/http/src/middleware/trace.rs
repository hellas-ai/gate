//! Request tracing layer factory.

use crate::middleware::CORRELATION_ID_HEADER;
use axum::{
    Router,
    http::{Request, Response},
};
use std::time::Duration;
use tower_http::{classify::ServerErrorsFailureClass, trace::TraceLayer};
use tracing::Span;

/// Add a `TraceLayer` configured to log completion status and latency.
/// - INFO for < 400, WARN for 4xx, ERROR for 5xx
/// - Includes correlation id header if present
pub fn with_request_tracing<State>(router: Router<State>) -> Router<State>
where
    State: Clone + Send + Sync + 'static,
{
    let layer = TraceLayer::new_for_http()
        .make_span_with(|req: &Request<_>| {
            let method = req.method();
            let path = req.uri().path();
            info_span!("http", %method, %path)
        })
        .on_response(|res: &Response<_>, latency: Duration, span: &Span| {
            let status = res.status();
            let cid = res
                .headers()
                .get(CORRELATION_ID_HEADER)
                .and_then(|v| v.to_str().ok());
            span.in_scope(|| {
                if status.is_server_error() {
                    if let Some(cid) = cid {
                        error!(%status, elapsed_ms = latency.as_millis(), correlation_id = %cid, "request completed");
                    } else {
                        error!(%status, elapsed_ms = latency.as_millis(), "request completed");
                    }
                } else if status.is_client_error() {
                    if let Some(cid) = cid {
                        warn!(%status, elapsed_ms = latency.as_millis(), correlation_id = %cid, "request completed");
                    } else {
                        warn!(%status, elapsed_ms = latency.as_millis(), "request completed");
                    }
                } else if let Some(cid) = cid {
                    info!(%status, elapsed_ms = latency.as_millis(), correlation_id = %cid, "request completed");
                } else {
                    info!(%status, elapsed_ms = latency.as_millis(), "request completed");
                }
            })
        })
        .on_failure(|error: ServerErrorsFailureClass, latency: Duration, span: &Span| {
            span.in_scope(|| {
                match error {
                    ServerErrorsFailureClass::StatusCode(status) if status.is_client_error() => {
                        warn!(%status, elapsed_ms = latency.as_millis(), "request failed");
                    }
                    ServerErrorsFailureClass::StatusCode(status) => {
                        error!(%status, elapsed_ms = latency.as_millis(), "request failed");
                    }
                    other => {
                        error!(failure = ?other, elapsed_ms = latency.as_millis(), "request failed");
                    }
                }
            })
        });

    router.layer(layer)
}
