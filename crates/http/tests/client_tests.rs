//! Integration tests for the Gate HTTP client

#![cfg(feature = "client")]

use gate_http::client::{GateClient, error::ClientError};
use gate_http::types::AnthropicMessagesRequest;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_client_builder() {
    let client = GateClient::builder()
        .base_url("http://localhost:8080")
        .api_key("test-key")
        .build();

    assert!(client.is_ok());
    let client = client.unwrap();
    assert_eq!(client.base_url().as_str(), "http://localhost:8080/");
}

#[tokio::test]
async fn test_client_builder_requires_base_url() {
    let result = GateClient::builder().build();
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
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .mount(&mock_server)
        .await;

    let client = GateClient::new(mock_server.uri()).unwrap();

    let request = AnthropicMessagesRequest {
        model: "claude-3".to_string(),
        stream: false,
        extra: None,
    };

    let response = client.messages(request).await.unwrap();
    assert_eq!(response["id"], "msg_123");
    assert_eq!(response["role"], "assistant");
}

#[tokio::test]
async fn test_auth_with_api_key() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("authorization", "Bearer test-api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&mock_server)
        .await;

    let client = GateClient::builder()
        .base_url(mock_server.uri())
        .api_key("test-api-key")
        .build()
        .unwrap();

    let request = AnthropicMessagesRequest {
        model: "claude-3".to_string(),
        stream: false,
        extra: None,
    };

    let response = client.messages(request).await;
    assert!(response.is_ok());
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

    let client = GateClient::new(mock_server.uri()).unwrap();

    let request = AnthropicMessagesRequest {
        model: "claude-3".to_string(),
        stream: false,
        extra: None,
    };

    let result = client.messages(request).await;
    assert!(matches!(result, Err(ClientError::AuthenticationFailed(_))));
}
