//! API route definitions
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;

pub mod health;
pub mod inference;
pub mod models;
pub mod observability;

#[derive(OpenApi)]
#[openapi(
    components(
        schemas()
    ),
    tags(
        (name = "api_keys", description = "API key management endpoints"),
        (name = "users", description = "User management endpoints"),
    ),
)]
struct ApiDoc;

pub fn router<T>() -> utoipa_axum::router::OpenApiRouter<crate::AppState<T>>
where
    T: Send + Sync + Clone + 'static,
{
    // Create the base router
    let api = ApiDoc::openapi();
    OpenApiRouter::with_openapi(api.clone())
}
