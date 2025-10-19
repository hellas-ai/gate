//! Integration tests for the Gate HTTP client

#![cfg(feature = "client")]

use gate_http::client::inference::{AnthropicMessage, MessageRequest};
use gate_http::client::{
    AuthenticatedGateClient, ClientBuilder, PublicGateClient, error::ClientError,
};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_client_builder() {
    // Test building a public client
    let public_client = ClientBuilder::default()
        .base_url("http://localhost:8080")
        .build_public();

    assert!(public_client.is_ok());

    // Test building an authenticated client
    let auth_client = ClientBuilder::default()
        .base_url("http://localhost:8080")
        .build_authenticated("test-key");

    assert!(auth_client.is_ok());
}

#[tokio::test]
async fn test_client_builder_requires_base_url() {
    let result = ClientBuilder::default().build_public();
    assert!(matches!(result, Err(ClientError::Configuration(_))));
}

#[tokio::test]
async fn test_anthropic_messages_endpoint() {
    let mock_server = MockServer::start().await;

    let response_body = json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [{
            "type": "text",
            "text": "Hello from test!"
        }],
        "model": "claude-3",
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": {
            "input_tokens": 10,
            "output_tokens": 15
        }
    });

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("content-type", "application/json"))
        .and(header("authorization", "Bearer test-api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .mount(&mock_server)
        .await;

    let client = AuthenticatedGateClient::new(mock_server.uri(), "test-api-key").unwrap();

    let request = MessageRequest {
        model: "claude-3".to_string(),
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
        }],
        max_tokens: 100,
        temperature: None,
        stream: Some(false),
        system: None,
    };

    let response = client.create_message(request).await.unwrap();
    assert_eq!(response.id, "msg_123");
    assert_eq!(response.role, "assistant");
}

#[tokio::test]
async fn test_public_and_authenticated_clients() {
    let mock_server = MockServer::start().await;

    // Test public endpoint
    Mock::given(method("GET"))
        .and(path("/public/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
        .mount(&mock_server)
        .await;

    let public_client = PublicGateClient::new(mock_server.uri()).unwrap();
    let request = public_client
        .request(reqwest::Method::GET, "/public/health")
        .unwrap();
    let response = public_client
        .execute::<serde_json::Value>(request)
        .await
        .unwrap();
    assert_eq!(response["status"], "ok");

    // Test authenticated endpoint
    Mock::given(method("GET"))
        .and(path("/api/user"))
        .and(header("authorization", "Bearer test-api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "user123"})))
        .mount(&mock_server)
        .await;

    let auth_client = AuthenticatedGateClient::new(mock_server.uri(), "test-api-key").unwrap();
    let request = auth_client
        .request(reqwest::Method::GET, "/api/user")
        .unwrap();
    let response = auth_client
        .execute::<serde_json::Value>(request)
        .await
        .unwrap();
    assert_eq!(response["id"], "user123");
}

#[tokio::test]
async fn test_error_handling() {
    let mock_server = MockServer::start().await;

    // Test 401 Unauthorized
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
        .mount(&mock_server)
        .await;

    let client = AuthenticatedGateClient::new(mock_server.uri(), "wrong-key").unwrap();

    let request = MessageRequest {
        model: "claude-3".to_string(),
        messages: vec![AnthropicMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
        }],
        max_tokens: 100,
        temperature: None,
        stream: Some(false),
        system: None,
    };

    let result = client.create_message(request).await;
    assert!(matches!(result, Err(ClientError::AuthenticationFailed(_))));
}
