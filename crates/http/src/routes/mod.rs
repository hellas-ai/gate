//! API route definitions

pub mod health;
pub mod inference;
pub mod models;
pub mod observability;

use axum::Router;

pub fn router<T>() -> Router<crate::AppState<T>>
where
    T: Send + Sync + Clone + 'static,
{
    Router::new()
        .merge(health::router())
        .merge(inference::router())
        .merge(models::router())
        .merge(observability::router())
}
