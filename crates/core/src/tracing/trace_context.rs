//! W3C Trace Context implementation
//!
//! This module implements the W3C Trace Context specification for distributed tracing.
//! See: https://www.w3.org/TR/trace-context/

use std::fmt;
use std::str::FromStr;

use http::{HeaderMap, HeaderName, HeaderValue};
use thiserror::Error;

/// W3C Trace Context header names
pub const TRACEPARENT_HEADER: &str = "traceparent";
pub const TRACESTATE_HEADER: &str = "tracestate";

/// Errors that can occur when parsing trace context
#[derive(Error, Debug)]
pub enum TraceContextError {
    #[error("Invalid traceparent format")]
    InvalidFormat,
    #[error("Invalid version: {0}")]
    InvalidVersion(String),
    #[error("Invalid trace ID")]
    InvalidTraceId,
    #[error("Invalid span ID")]
    InvalidSpanId,
    #[error("Invalid flags")]
    InvalidFlags,
    #[error("Invalid header value: {0}")]
    InvalidHeaderValue(String),
}

/// W3C Trace Context
///
/// Represents the traceparent header as defined in the W3C Trace Context specification.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TraceContext {
    version: u8,
    trace_id: [u8; 16],
    span_id: [u8; 8],
    flags: u8,
}

impl TraceContext {
    /// Create a new trace context with random IDs
    pub fn new() -> Self {
        let mut trace_id = [0u8; 16];
        let mut span_id = [0u8; 8];

        // Use getrandom for WASM compatibility
        #[cfg(target_arch = "wasm32")]
        {
            getrandom::getrandom(&mut trace_id).expect("Failed to generate random trace ID");
            getrandom::getrandom(&mut span_id).expect("Failed to generate random span ID");
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            use std::time::{SystemTime, UNIX_EPOCH};

            // Generate pseudo-random bytes using system time and thread ID
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();

            let mut hasher = DefaultHasher::new();
            now.hash(&mut hasher);
            std::thread::current().id().hash(&mut hasher);
            let hash1 = hasher.finish();

            hasher = DefaultHasher::new();
            hash1.hash(&mut hasher);
            now.wrapping_add(1).hash(&mut hasher);
            let hash2 = hasher.finish();

            trace_id[..8].copy_from_slice(&hash1.to_be_bytes());
            trace_id[8..].copy_from_slice(&hash2.to_be_bytes());
            span_id.copy_from_slice(&hash1.to_le_bytes());
        }

        Self {
            version: 0,
            trace_id,
            span_id,
            flags: 1, // Sampled by default
        }
    }

    /// Create a trace context from a legacy correlation ID
    pub fn from_legacy_id(id: &str) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Generate deterministic trace and span IDs from the legacy ID
        let mut hasher = DefaultHasher::new();
        id.hash(&mut hasher);
        let hash1 = hasher.finish();

        hasher = DefaultHasher::new();
        id.hash(&mut hasher);
        "trace".hash(&mut hasher);
        let hash2 = hasher.finish();

        let mut trace_id = [0u8; 16];
        trace_id[..8].copy_from_slice(&hash1.to_be_bytes());
        trace_id[8..].copy_from_slice(&hash2.to_be_bytes());

        let mut span_id = [0u8; 8];
        span_id.copy_from_slice(&hash1.to_le_bytes());

        Self {
            version: 0,
            trace_id,
            span_id,
            flags: 1, // Sampled by default
        }
    }

    /// Get the trace ID
    pub fn trace_id(&self) -> &[u8; 16] {
        &self.trace_id
    }

    /// Get the span ID
    pub fn span_id(&self) -> &[u8; 8] {
        &self.span_id
    }

    /// Check if the trace is sampled
    pub fn is_sampled(&self) -> bool {
        self.flags & 0x01 != 0
    }

    /// Set the sampled flag
    pub fn set_sampled(&mut self, sampled: bool) {
        if sampled {
            self.flags |= 0x01;
        } else {
            self.flags &= !0x01;
        }
    }

    /// Create a child context with a new span ID
    pub fn child(&self) -> Self {
        let mut child = self.clone();

        // Generate new span ID
        #[cfg(target_arch = "wasm32")]
        {
            getrandom::getrandom(&mut child.span_id).expect("Failed to generate random span ID");
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            use std::time::{SystemTime, UNIX_EPOCH};

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();

            let mut hasher = DefaultHasher::new();
            now.hash(&mut hasher);
            self.span_id.hash(&mut hasher);
            let hash = hasher.finish();

            child.span_id.copy_from_slice(&hash.to_be_bytes());
        }

        child
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TraceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02x}-{}-{}-{:02x}",
            self.version,
            hex::encode(self.trace_id),
            hex::encode(self.span_id),
            self.flags
        )
    }
}

impl FromStr for TraceContext {
    type Err = TraceContextError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Format: version-traceid-spanid-flags
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 4 {
            return Err(TraceContextError::InvalidFormat);
        }

        // Parse version
        let version = u8::from_str_radix(parts[0], 16)
            .map_err(|_| TraceContextError::InvalidVersion(parts[0].to_string()))?;

        // We only support version 00
        if version != 0 {
            return Err(TraceContextError::InvalidVersion(format!("{version:02x}")));
        }

        // Parse trace ID (32 hex chars = 16 bytes)
        if parts[1].len() != 32 {
            return Err(TraceContextError::InvalidTraceId);
        }
        let trace_id_vec = hex::decode(parts[1]).map_err(|_| TraceContextError::InvalidTraceId)?;
        let mut trace_id = [0u8; 16];
        trace_id.copy_from_slice(&trace_id_vec);

        // Validate trace ID is not all zeros
        if trace_id.iter().all(|&b| b == 0) {
            return Err(TraceContextError::InvalidTraceId);
        }

        // Parse span ID (16 hex chars = 8 bytes)
        if parts[2].len() != 16 {
            return Err(TraceContextError::InvalidSpanId);
        }
        let span_id_vec = hex::decode(parts[2]).map_err(|_| TraceContextError::InvalidSpanId)?;
        let mut span_id = [0u8; 8];
        span_id.copy_from_slice(&span_id_vec);

        // Validate span ID is not all zeros
        if span_id.iter().all(|&b| b == 0) {
            return Err(TraceContextError::InvalidSpanId);
        }

        // Parse flags
        let flags =
            u8::from_str_radix(parts[3], 16).map_err(|_| TraceContextError::InvalidFlags)?;

        Ok(Self {
            version,
            trace_id,
            span_id,
            flags,
        })
    }
}

/// Extract trace context from HTTP headers
pub fn extract_trace_context(headers: &HeaderMap) -> Option<TraceContext> {
    headers
        .get(TRACEPARENT_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| TraceContext::from_str(s).ok())
}

/// Inject trace context into HTTP headers
pub fn inject_trace_context(
    ctx: &TraceContext,
    headers: &mut HeaderMap,
) -> Result<(), TraceContextError> {
    let traceparent = ctx.to_string();
    let header_value = HeaderValue::from_str(&traceparent)
        .map_err(|_| TraceContextError::InvalidHeaderValue(traceparent))?;

    headers.insert(HeaderName::from_static(TRACEPARENT_HEADER), header_value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_trace_context() {
        let ctx1 = TraceContext::new();
        let ctx2 = TraceContext::new();

        // Should have different IDs
        assert_ne!(ctx1.trace_id, ctx2.trace_id);
        assert_ne!(ctx1.span_id, ctx2.span_id);

        // Should be sampled by default
        assert!(ctx1.is_sampled());
        assert!(ctx2.is_sampled());
    }

    #[test]
    fn test_parse_traceparent() {
        let traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let ctx = TraceContext::from_str(traceparent).unwrap();

        assert_eq!(ctx.version, 0);
        assert_eq!(
            hex::encode(ctx.trace_id),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
        assert_eq!(hex::encode(ctx.span_id), "00f067aa0ba902b7");
        assert_eq!(ctx.flags, 1);
        assert!(ctx.is_sampled());

        // Should round-trip
        assert_eq!(ctx.to_string(), traceparent);
    }

    #[test]
    fn test_invalid_traceparent() {
        // Wrong number of parts
        assert!(TraceContext::from_str("00-invalid").is_err());

        // Invalid version
        assert!(
            TraceContext::from_str("ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
                .is_err()
        );

        // Invalid trace ID (wrong length)
        assert!(TraceContext::from_str("00-4bf92f3577b34da6-00f067aa0ba902b7-01").is_err());

        // Invalid trace ID (all zeros)
        assert!(
            TraceContext::from_str("00-00000000000000000000000000000000-00f067aa0ba902b7-01")
                .is_err()
        );

        // Invalid span ID (wrong length)
        assert!(TraceContext::from_str("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa-01").is_err());

        // Invalid span ID (all zeros)
        assert!(
            TraceContext::from_str("00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01")
                .is_err()
        );
    }

    #[test]
    fn test_child_context() {
        let parent = TraceContext::new();
        let child = parent.child();

        // Should have same trace ID
        assert_eq!(parent.trace_id, child.trace_id);

        // Should have different span ID
        assert_ne!(parent.span_id, child.span_id);

        // Should inherit flags
        assert_eq!(parent.flags, child.flags);
    }

    #[test]
    fn test_from_legacy_id() {
        let legacy = "my-correlation-id";
        let ctx1 = TraceContext::from_legacy_id(legacy);
        let ctx2 = TraceContext::from_legacy_id(legacy);

        // Should be deterministic
        assert_eq!(ctx1.trace_id, ctx2.trace_id);
        assert_eq!(ctx1.span_id, ctx2.span_id);

        // Should be sampled
        assert!(ctx1.is_sampled());
    }

    #[test]
    fn test_extract_inject_headers() {
        let ctx = TraceContext::new();
        let mut headers = HeaderMap::new();

        // Inject
        inject_trace_context(&ctx, &mut headers).unwrap();
        assert!(headers.contains_key(TRACEPARENT_HEADER));

        // Extract
        let extracted = extract_trace_context(&headers).unwrap();
        assert_eq!(ctx, extracted);
    }
}
