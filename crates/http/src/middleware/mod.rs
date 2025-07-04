//! Middleware components for HTTP request processing

pub mod auth;
pub mod correlation;
pub mod metrics;
pub mod rbac;
pub mod trace;
#[cfg(not(target_arch = "wasm32"))]
pub mod webauthn;

#[cfg(not(target_arch = "wasm32"))]
pub use auth::ServiceAuthProvider;
pub use auth::{AuthProvider, AuthenticatedUser, auth_middleware};
pub use correlation::{
    CORRELATION_ID_HEADER, CorrelationIdExt, correlation_id_middleware, extract_correlation_id,
};
pub use metrics::metrics_middleware;
pub use rbac::{RequiredRoles, check_admin_role, check_roles};
pub use trace::TraceMiddleware;
#[cfg(not(target_arch = "wasm32"))]
pub use webauthn::{WebAuthnConfig, WebAuthnState};
