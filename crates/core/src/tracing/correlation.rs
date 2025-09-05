//! Correlation ID for distributed tracing
//!
//! This module provides a correlation ID that wraps W3C TraceContext
//! for distributed tracing while supporting legacy string-based correlation IDs.

use std::{fmt, str::FromStr, sync::Arc};

use crate::tracing::trace_context::{TraceContext, TraceContextError};

/// A correlation ID for distributed tracing
///
/// This wraps the W3C TraceContext standard while maintaining compatibility
/// with legacy string-based correlation IDs.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CorrelationId {
    /// The underlying W3C trace context
    trace_context: Arc<TraceContext>,
    /// Optional legacy correlation ID for backward compatibility
    legacy_id: Option<String>,
}

impl CorrelationId {
    /// Create a new correlation ID with a random trace context
    pub fn new() -> Self {
        Self {
            trace_context: Arc::new(TraceContext::new()),
            legacy_id: None,
        }
    }

    /// Create from a string (tries to parse as traceparent first, then as legacy ID)
    pub fn from_string(s: &str) -> Self {
        if let Ok(ctx) = TraceContext::from_str(s) {
            Self {
                trace_context: Arc::new(ctx),
                legacy_id: None,
            }
        } else {
            Self::from_legacy(s)
        }
    }

    /// Create from a W3C traceparent string
    pub fn from_traceparent(traceparent: &str) -> Result<Self, TraceContextError> {
        Ok(Self {
            trace_context: Arc::new(TraceContext::from_str(traceparent)?),
            legacy_id: None,
        })
    }

    /// Create from a TraceContext
    pub fn from_trace_context(ctx: TraceContext) -> Self {
        Self {
            trace_context: Arc::new(ctx),
            legacy_id: None,
        }
    }

    /// Create from a legacy correlation ID string
    pub fn from_legacy(id: &str) -> Self {
        // Create a deterministic trace context from the legacy ID
        let ctx = TraceContext::from_legacy_id(id);
        Self {
            trace_context: Arc::new(ctx),
            legacy_id: Some(id.to_string()),
        }
    }

    /// Get the trace ID as a hex string
    pub fn trace_id(&self) -> String {
        hex::encode(self.trace_context.trace_id())
    }

    /// Get the span ID as a hex string
    pub fn span_id(&self) -> String {
        hex::encode(self.trace_context.span_id())
    }

    /// Get the W3C traceparent header value
    pub fn to_traceparent(&self) -> String {
        self.trace_context.to_string()
    }

    /// Get the underlying trace context
    pub fn trace_context(&self) -> &TraceContext {
        &self.trace_context
    }

    /// Convert to OpenTelemetry SpanContext
    #[cfg(all(feature = "tracing-otlp", not(target_arch = "wasm32")))]
    #[cfg_attr(
        docsrs,
        doc(cfg(all(feature = "tracing-otlp", not(target_arch = "wasm32"))))
    )]
    pub fn to_span_context(&self) -> opentelemetry::trace::SpanContext {
        opentelemetry::trace::SpanContext::new(
            opentelemetry::trace::TraceId::from_bytes(*self.trace_context.trace_id()),
            opentelemetry::trace::SpanId::from_bytes(*self.trace_context.span_id()),
            if self.trace_context.is_sampled() {
                opentelemetry::trace::TraceFlags::SAMPLED
            } else {
                opentelemetry::trace::TraceFlags::default()
            },
            false,
            opentelemetry::trace::TraceState::default(),
        )
    }

    /// Create a child correlation ID with a new span ID
    pub fn child(&self) -> Self {
        Self {
            trace_context: Arc::new(self.trace_context.child()),
            legacy_id: self.legacy_id.clone(),
        }
    }
}

impl Default for CorrelationId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for CorrelationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // If we have a legacy ID, use that for display
        if let Some(ref legacy) = self.legacy_id {
            write!(f, "{legacy}")
        } else {
            // Otherwise use the trace ID
            write!(f, "{}", self.trace_id())
        }
    }
}

impl FromStr for CorrelationId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_string(s))
    }
}

// Thread-local storage for current correlation ID (native only)
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static CURRENT_CORRELATION_ID: RefCell<Option<CorrelationId>> = const { RefCell::new(None) };
    }

    /// Get the current correlation ID for this thread
    pub fn current_correlation_id() -> Option<CorrelationId> {
        CURRENT_CORRELATION_ID.with(|id| id.borrow().clone())
    }

    /// Set the current correlation ID for this thread
    pub fn set_current_correlation_id(id: Option<CorrelationId>) {
        CURRENT_CORRELATION_ID.with(|current| {
            *current.borrow_mut() = id;
        });
    }

    /// RAII guard for setting correlation ID in a scope
    pub struct CorrelationScope {
        previous: Option<CorrelationId>,
    }

    impl CorrelationScope {
        /// Create a new correlation scope
        pub fn new(id: CorrelationId) -> Self {
            let previous = current_correlation_id();
            set_current_correlation_id(Some(id));
            Self { previous }
        }
    }

    impl Drop for CorrelationScope {
        fn drop(&mut self) {
            set_current_correlation_id(self.previous.take());
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{CorrelationScope, current_correlation_id, set_current_correlation_id};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_correlation_id() {
        let id1 = CorrelationId::new();
        let id2 = CorrelationId::new();
        assert_ne!(id1, id2);
        assert_ne!(id1.trace_id(), id2.trace_id());
    }

    #[test]
    fn test_from_traceparent() {
        let traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let id = CorrelationId::from_traceparent(traceparent).unwrap();
        assert_eq!(id.to_traceparent(), traceparent);
        assert_eq!(id.trace_id(), "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(id.span_id(), "00f067aa0ba902b7");
    }

    #[test]
    fn test_from_legacy() {
        let legacy = "my-correlation-id";
        let id = CorrelationId::from_legacy(legacy);
        assert_eq!(id.to_string(), legacy);
        // Should create a deterministic trace context
        let id2 = CorrelationId::from_legacy(legacy);
        assert_eq!(id.trace_id(), id2.trace_id());
    }

    #[test]
    fn test_child() {
        let parent = CorrelationId::new();
        let child = parent.child();
        assert_eq!(parent.trace_id(), child.trace_id());
        assert_ne!(parent.span_id(), child.span_id());
    }

    #[test]
    fn test_from_string() {
        // Should parse traceparent
        let traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let id = CorrelationId::from_string(traceparent);
        assert_eq!(id.to_traceparent(), traceparent);

        // Should treat as legacy ID
        let legacy = "not-a-traceparent";
        let id = CorrelationId::from_string(legacy);
        assert_eq!(id.to_string(), legacy);
    }
}
