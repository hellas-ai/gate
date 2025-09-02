//! Integration tests for HTTP routes

use axum::{
    body::Body,
    http::{HeaderMap, HeaderName, HeaderValue, Request, StatusCode},
};
use gate_http::auth::extract_identity;
use serde_json::json;

#[tokio::test]
async fn test_identity_extraction_with_bearer() {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_static("Bearer test-jwt-token"),
    );

    let identity = extract_identity(&headers);
    assert_eq!(identity.source, "bearer");
    assert!(!identity.id.is_empty());
}

#[tokio::test]
async fn test_identity_extraction_with_basic_auth() {
    let mut headers = HeaderMap::new();
    // "testuser:testpass" in base64
    headers.insert(
        HeaderName::from_static("authorization"),
        HeaderValue::from_static("Basic dGVzdHVzZXI6dGVzdHBhc3M="),
    );

    let identity = extract_identity(&headers);
    assert_eq!(identity.source, "basic");
    assert_eq!(identity.id, "testuser");
}

#[tokio::test]
async fn test_identity_extraction_with_api_key() {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("x-api-key"),
        HeaderValue::from_static("sk-test-12345"),
    );

    let identity = extract_identity(&headers);
    assert_eq!(identity.source, "api_key");
    assert!(identity.context.api_key_hash.is_some());
}

#[tokio::test]
async fn test_identity_extraction_anonymous() {
    let headers = HeaderMap::new();
    let identity = extract_identity(&headers);
    assert_eq!(identity.source, "http");
    assert_eq!(identity.id, "anonymous");
}

#[tokio::test]
async fn test_sse_parser() {
    use bytes::Bytes;
    use futures::{StreamExt, stream};
    use gate_http::sinks::sse_parser::parse_sse;

    let data = b"event: message\ndata: {\"text\": \"hello\"}\nid: 123\n\n";
    let stream = stream::once(async { Ok::<_, reqwest::Error>(Bytes::from(&data[..])) });
    let stream = Box::pin(stream);
    let mut parser = parse_sse(stream);

    let event = parser.next().await.unwrap().unwrap();
    assert_eq!(event.event, Some("message".to_string()));
    assert_eq!(event.data, "{\"text\": \"hello\"}");
    assert_eq!(event.id, Some("123".to_string()));
}

#[tokio::test]
async fn test_sse_parser_multiline() {
    use bytes::Bytes;
    use futures::{StreamExt, stream};
    use gate_http::sinks::sse_parser::parse_sse;

    let data = b"data: line 1\ndata: line 2\ndata: line 3\n\n";
    let stream = stream::once(async { Ok::<_, reqwest::Error>(Bytes::from(&data[..])) });
    let stream = Box::pin(stream);
    let mut parser = parse_sse(stream);

    let event = parser.next().await.unwrap().unwrap();
    assert_eq!(event.data, "line 1\nline 2\nline 3");
}

#[tokio::test]
async fn test_sse_parser_split_chunks() {
    use bytes::Bytes;
    use futures::{StreamExt, stream};
    use gate_http::sinks::sse_parser::parse_sse;

    // Simulate data arriving in multiple chunks
    let chunk1 = b"data: {\"partial\":";
    let chunk2 = b"\"message\"}\n\n";

    let stream = stream::iter(vec![
        Ok::<_, reqwest::Error>(Bytes::from(&chunk1[..])),
        Ok(Bytes::from(&chunk2[..])),
    ]);
    let stream = Box::pin(stream);

    let mut parser = parse_sse(stream);

    let event = parser.next().await.unwrap().unwrap();
    assert_eq!(event.data, "{\"partial\":\"message\"}");
}

#[tokio::test]
async fn test_sse_parser_with_comments() {
    use bytes::Bytes;
    use futures::{StreamExt, stream};
    use gate_http::sinks::sse_parser::parse_sse;

    let data = b": this is a comment\ndata: actual data\n\n";
    let stream = stream::once(async { Ok::<_, reqwest::Error>(Bytes::from(&data[..])) });
    let stream = Box::pin(stream);
    let mut parser = parse_sse(stream);

    let event = parser.next().await.unwrap().unwrap();
    assert_eq!(event.data, "actual data");
}

#[tokio::test]
async fn test_sse_parser_with_retry() {
    use bytes::Bytes;
    use futures::{StreamExt, stream};
    use gate_http::sinks::sse_parser::parse_sse;

    let data = b"retry: 5000\ndata: test message\n\n";
    let stream = stream::once(async { Ok::<_, reqwest::Error>(Bytes::from(&data[..])) });
    let stream = Box::pin(stream);
    let mut parser = parse_sse(stream);

    let event = parser.next().await.unwrap().unwrap();
    assert_eq!(event.retry, Some(5000));
    assert_eq!(event.data, "test message");
}
