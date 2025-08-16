//! Router construction module for the daemon
//!
//! This module provides a clean abstraction for building the daemon's HTTP router,
//! separating route configuration from the daemon builder logic.

use crate::MinimalState;
use gate_http::AppState;
use utoipa_axum::router::OpenApiRouter;

/// Build the complete daemon router with all routes
pub fn build_router() -> OpenApiRouter<AppState<MinimalState>> {
    let router = OpenApiRouter::new();

    // Add auth routes
    let router = crate::routes::auth::add_routes(router);

    // Add config routes
    let router = crate::routes::config::add_routes(router);

    // Add admin routes
    crate::routes::admin::add_routes(router)
}

/// Build a router for testing with only specific route groups
#[cfg(test)]
pub fn build_test_router(
    include_auth: bool,
    include_config: bool,
    include_admin: bool,
) -> OpenApiRouter<AppState<MinimalState>> {
    let mut router = OpenApiRouter::new();

    if include_auth {
        router = crate::routes::auth::add_routes(router);
    }

    if include_config {
        router = crate::routes::config::add_routes(router);
    }

    if include_admin {
        router = crate::routes::admin::add_routes(router);
    }

    router
}
