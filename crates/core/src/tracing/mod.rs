//! Shared tracing functionality for Gate
//!
//! This module provides common tracing utilities that work across both
//! WASM and native environments.

pub mod correlation;
pub mod metrics;
pub mod trace_context;

#[cfg(all(feature = "tracing-otlp", not(target_arch = "wasm32")))]
pub mod config;
#[cfg(all(feature = "tracing-otlp", not(target_arch = "wasm32")))]
pub mod init;

#[cfg(all(feature = "tracing-prometheus", not(target_arch = "wasm32")))]
pub mod prometheus;

// Re-export commonly used types
pub use correlation::CorrelationId;
pub use trace_context::{TraceContext, TraceContextError};

pub mod prelude {
    pub use crate::tracing::correlation::CorrelationId;
    pub use crate::tracing::metrics::{
        Counter, Gauge, Histogram, Timer, counter, gauge, histogram, log_all_metrics,
    };
    pub use crate::tracing::trace_context::{
        TraceContext, TraceContextError, extract_trace_context, inject_trace_context,
    };

    // Re-export common tracing macros and types
    pub use tracing::{Instrument, debug, error, info, instrument, trace, warn};

    // Native-only exports
    #[cfg(not(target_arch = "wasm32"))]
    pub use crate::tracing::correlation::{
        CorrelationScope, current_correlation_id, set_current_correlation_id,
    };
}
