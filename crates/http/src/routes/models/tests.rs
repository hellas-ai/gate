#[cfg(test)]
use crate::forwarding::{ForwardingConfig, UpstreamProvider, UpstreamRegistry};
use crate::state::AppState;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use gate_core::tests::state::InMemoryBackend;
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn test_models_endpoint() {
    // Create a test upstream registry
    let registry = Arc::new(UpstreamRegistry::new());

    // Register some test upstreams with models
    let config1 = ForwardingConfig {
        provider: UpstreamProvider::OpenAI,
        base_url: "http://test1.com".to_string(),
        api_key: Some("test".to_string()),
        timeout_seconds: 30,
    };

    let config2 = ForwardingConfig {
        provider: UpstreamProvider::OpenAI,
        base_url: "http://test2.com".to_string(),
        api_key: Some("test".to_string()),
        timeout_seconds: 30,
    };

    registry
        .register_upstream(
            "test-upstream-1".to_string(),
            config1,
            vec!["model1".to_string(), "model2".to_string()],
        )
        .await;

    registry
        .register_upstream(
            "test-upstream-2".to_string(),
            config2,
            vec!["model3".to_string(), "model4".to_string()],
        )
        .await;

    // Create test app state with in-memory backend
    let state_backend = Arc::new(InMemoryBackend::default());
    let app_state = AppState::<()>::new(state_backend, ()).with_upstream_registry(registry);

    // Create router with models endpoint
    let app = axum::Router::new()
        .route(
            "/v1/models",
            axum::routing::get(crate::routes::models::models_handler),
        )
        .with_state(app_state);

    // Make request to /v1/models
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Check response
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), 10_000_000).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["object"], "list");
    assert_eq!(json["data"].as_array().unwrap().len(), 4);

    // Check that all models are present
    let models: Vec<String> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();

    assert!(models.contains(&"model1".to_string()));
    assert!(models.contains(&"model2".to_string()));
    assert!(models.contains(&"model3".to_string()));
    assert!(models.contains(&"model4".to_string()));
}
