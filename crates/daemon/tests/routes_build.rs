use axum::Router;
use gate_daemon::{
    State,
    routes::{admin, auth, config},
};

// Ensure admin routes construct without panicking (e.g., invalid path syntax)
#[test]
fn admin_routes_builds() {
    let _ = admin::add_routes(Router::<gate_http::AppState<State>>::new());
}

// Ensure auth routes construct without panicking
#[test]
fn auth_routes_builds() {
    let _ = auth::add_routes(Router::<gate_http::AppState<State>>::new());
}

// Ensure config routes construct without panicking
#[test]
fn config_routes_builds() {
    let _ = config::add_routes(Router::<gate_http::AppState<State>>::new());
}
