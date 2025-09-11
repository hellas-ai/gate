use axum::Router;

// Ensure top-level router constructs without overlapping routes or panics
#[test]
fn http_routes_build() {
    let _r: Router<gate_http::AppState<()>> = gate_http::routes::router::<()>();
}
