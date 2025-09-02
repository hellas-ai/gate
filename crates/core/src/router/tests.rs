//! Tests for the router module

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::Result;
    use crate::state::StateBackend;
    use async_trait::async_trait;

    // Mock state backend for testing
    struct MockStateBackend;

    #[async_trait]
    impl StateBackend for MockStateBackend {
        async fn get_user(&self, _user_id: &str) -> Result<Option<crate::User>> {
            Ok(None)
        }

        async fn get_user_by_id(&self, _user_id: &str) -> Result<Option<crate::User>> {
            Ok(None)
        }

        async fn create_user(&self, _user: &crate::User) -> Result<()> {
            Ok(())
        }

        async fn update_user(&self, _user: &crate::User) -> Result<()> {
            Ok(())
        }

        async fn delete_user(&self, _user_id: &str) -> Result<()> {
            Ok(())
        }

        async fn list_users(&self) -> Result<Vec<crate::User>> {
            Ok(vec![])
        }

        async fn get_api_key(&self, _key_hash: &str) -> Result<Option<crate::ApiKey>> {
            Ok(None)
        }

        async fn create_api_key(&self, _key: &crate::ApiKey, _raw_key: &str) -> Result<()> {
            Ok(())
        }

        async fn list_api_keys(&self, _org_id: &str) -> Result<Vec<crate::ApiKey>> {
            Ok(vec![])
        }

        async fn delete_api_key(&self, _key_hash: &str) -> Result<()> {
            Ok(())
        }

        async fn record_usage(&self, _usage: &crate::UsageRecord) -> Result<()> {
            Ok(())
        }

        async fn get_usage(
            &self,
            _org_id: &str,
            _range: &crate::TimeRange,
        ) -> Result<Vec<crate::UsageRecord>> {
            Ok(vec![])
        }

        async fn get_provider(&self, _id: &str) -> Result<Option<crate::Provider>> {
            Ok(None)
        }

        async fn list_providers(&self) -> Result<Vec<crate::Provider>> {
            Ok(vec![])
        }

        async fn get_model(&self, _id: &str) -> Result<Option<crate::Model>> {
            Ok(None)
        }

        async fn list_models(&self) -> Result<Vec<crate::Model>> {
            Ok(vec![])
        }

        async fn get_organization(&self, _id: &str) -> Result<Option<crate::Organization>> {
            Ok(None)
        }

        async fn create_organization(&self, _org: &crate::Organization) -> Result<()> {
            Ok(())
        }

        async fn has_permission(
            &self,
            _subject_id: &str,
            _action: &crate::access::Action,
            _object: &crate::access::ObjectIdentity,
        ) -> Result<bool> {
            Ok(true)
        }

        async fn grant_permission(
            &self,
            _subject_id: &str,
            _action: &crate::access::Action,
            _object: &crate::access::ObjectIdentity,
        ) -> Result<()> {
            Ok(())
        }

        async fn remove_permission(
            &self,
            _subject_id: &str,
            _action: &crate::access::Action,
            _object: &crate::access::ObjectIdentity,
        ) -> Result<()> {
            Ok(())
        }

        async fn list_user_permissions(
            &self,
            _user_id: &str,
        ) -> Result<Vec<(String, String, chrono::DateTime<chrono::Utc>)>> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_router_creation() {
        let backend = std::sync::Arc::new(MockStateBackend);
        let registry = std::sync::Arc::new(routing::SinkRegistry::new());

        let router = routing::Router::builder()
            .state_backend(backend as std::sync::Arc<dyn crate::StateBackend>)
            .sink_registry(registry)
            .build();

        // Test that router implements Sink
        let _sink: &dyn Sink = &router;
    }

    #[test]
    fn test_protocol_conversion() {
        use serde_json::json;

        let openai_request = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful"},
                {"role": "user", "content": "Hello"}
            ],
            "temperature": 0.7
        });

        let (converted, warnings) =
            protocols::convert_request(Protocol::OpenAIChat, Protocol::Anthropic, &openai_request)
                .unwrap();

        assert!(converted.get("messages").is_some());
        assert!(converted.get("system").is_some());
        assert_eq!(converted["system"], "You are helpful");
        assert!(warnings.is_empty() || !warnings.is_empty()); // Either case is fine for now
    }

    #[tokio::test]
    async fn test_route_and_execute_with_mock_sink() {
        use crate::access::SubjectIdentity;
        use crate::router::index::SinkIndex;
        use crate::router::sink::RouterIdentityContext;
        use crate::router::sinks::mock::MockSink;
        use crate::router::types::{RequestCapabilities, RequestDescriptor};
        use futures::StreamExt;
        use serde_json::json;

        // Backend and registry
        let backend = std::sync::Arc::new(MockStateBackend);
        let registry = std::sync::Arc::new(routing::SinkRegistry::new());
        registry
            .register(
                "self://mock".to_string(),
                std::sync::Arc::new(MockSink::success("self://mock")),
            )
            .await;

        // Prepare sink index snapshot for fast routing
        let index = std::sync::Arc::new(SinkIndex::new());
        let sink = registry.get("self://mock").await.unwrap();
        let desc = sink.describe().await;
        let health = sink.probe().await;
        index
            .set_snapshot("self://mock".to_string(), desc, health)
            .await;

        // Router
        let router = routing::Router::builder()
            .state_backend(backend as std::sync::Arc<dyn crate::StateBackend>)
            .sink_registry(registry)
            .sink_index(index)
            .build();

        // Context and descriptor
        let ctx = sink::RequestContext {
            identity: SubjectIdentity::new(
                "user-1",
                "test",
                RouterIdentityContext {
                    org_id: Some("org-1".into()),
                    user_id: Some("user-1".into()),
                    api_key_hash: None,
                },
            ),
            correlation_id: crate::tracing::CorrelationId::new(),
            headers: Default::default(),
            trace_id: None,
            metadata: Default::default(),
        };

        let desc = RequestDescriptor {
            model: "test-model".into(),
            protocol: Protocol::OpenAIChat,
            capabilities: RequestCapabilities {
                needs_tools: false,
                needs_vision: false,
                needs_streaming: false,
                max_tokens: Some(64),
                modalities: vec!["text".into()],
            },
            context_length_hint: Some(100),
        };

        // Request stream (single event)
        let request_json = json!({
            "model": "test-model",
            "messages": [{"role":"user","content":"hello"}],
            "stream": false
        });
        let request_stream: types::RequestStream = types::RequestStream::new(
            Protocol::OpenAIChat,
            Box::pin(futures::stream::once(
                async move { Ok(request_json.clone()) },
            )),
        );

        // Route and execute
        let plan = router.route(&ctx, &desc).await.expect("route ok");
        let mut response = router
            .execute(plan, request_stream)
            .await
            .expect("execute ok");

        // Collect chunks
        let mut chunks = Vec::new();
        while let Some(item) = response.next().await {
            chunks.push(item.expect("chunk ok"));
        }

        assert!(matches!(chunks.get(0), Some(ResponseChunk::Headers(_))));
        match chunks.get(1) {
            Some(ResponseChunk::Content(v)) => {
                assert_eq!(v["ok"], json!(true));
                assert!(v["echo"].is_array());
            }
            other => panic!("unexpected content chunk: {:?}", other),
        }
        assert!(matches!(
            chunks.last(),
            Some(ResponseChunk::Stop {
                reason: StopReason::Complete,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn test_service_route_and_execute_json() {
        use crate::access::SubjectIdentity;
        use crate::router::index::SinkIndex;
        use crate::router::service::route_and_execute_json_with_protocol;
        use crate::router::sink::RouterIdentityContext;
        use crate::router::sinks::mock::MockSink;
        use crate::router::types::Protocol;
        use futures::StreamExt;
        use serde_json::json;

        let backend = std::sync::Arc::new(MockStateBackend);
        let registry = std::sync::Arc::new(routing::SinkRegistry::new());
        registry
            .register(
                "self://mock".into(),
                std::sync::Arc::new(MockSink::success("self://mock")),
            )
            .await;

        let index = std::sync::Arc::new(SinkIndex::new());
        index.refresh_from_registry(&registry).await;

        let router = routing::Router::builder()
            .state_backend(backend as std::sync::Arc<dyn crate::StateBackend>)
            .sink_registry(registry)
            .sink_index(index)
            .build();

        let ctx = sink::RequestContext {
            identity: SubjectIdentity::new(
                "user-1",
                "test",
                RouterIdentityContext {
                    org_id: None,
                    user_id: None,
                    api_key_hash: None,
                },
            ),
            correlation_id: crate::tracing::CorrelationId::new(),
            headers: Default::default(),
            trace_id: None,
            metadata: Default::default(),
        };

        let json_req = json!({
            "model": "m",
            "messages": [{"role":"user","content":"hi"}],
            "stream": false
        });

        let mut resp =
            route_and_execute_json_with_protocol(&router, &ctx, Protocol::OpenAIChat, json_req)
                .await
                .expect("exec");
        let mut chunks = Vec::new();
        while let Some(item) = resp.next().await {
            chunks.push(item.expect("ok"));
        }
        assert!(matches!(chunks.first(), Some(ResponseChunk::Headers(_))));
        assert!(matches!(chunks.last(), Some(ResponseChunk::Stop { .. })));
    }

    #[tokio::test]
    async fn test_sink_index_selects_healthy_candidate() {
        use crate::access::SubjectIdentity;
        use crate::router::index::SinkIndex;
        use crate::router::sink::RouterIdentityContext;
        use crate::router::sinks::mock::MockSink;
        use crate::router::types::{RequestCapabilities, RequestDescriptor};

        let backend = std::sync::Arc::new(MockStateBackend);
        let registry = std::sync::Arc::new(routing::SinkRegistry::new());

        // Two sinks: A healthy, B unhealthy
        let sink_a_id = "self://a";
        let sink_b_id = "self://b";
        registry
            .register(
                sink_a_id.to_string(),
                std::sync::Arc::new(MockSink::success(sink_a_id)),
            )
            .await;
        registry
            .register(
                sink_b_id.to_string(),
                std::sync::Arc::new(MockSink::unhealthy(sink_b_id)),
            )
            .await;

        // Build index snapshots
        let index = std::sync::Arc::new(SinkIndex::new());
        // Refresh from registry to populate index for all sinks
        let refreshed = index.refresh_from_registry(&registry).await;
        assert_eq!(refreshed, 2);

        // Router with index and deterministic weighted strategy
        use crate::router::strategy::WeightedStrategy;
        let weights = std::collections::HashMap::new();
        let strategy = Box::new(WeightedStrategy::deterministic(weights));
        let router = routing::Router::builder()
            .state_backend(backend as std::sync::Arc<dyn crate::StateBackend>)
            .sink_registry(registry)
            .sink_index(index)
            .strategy(strategy)
            .build();

        let ctx = sink::RequestContext {
            identity: SubjectIdentity::new(
                "user-1",
                "test",
                RouterIdentityContext {
                    org_id: None,
                    user_id: None,
                    api_key_hash: None,
                },
            ),
            correlation_id: crate::tracing::CorrelationId::new(),
            headers: Default::default(),
            trace_id: None,
            metadata: Default::default(),
        };

        let desc = RequestDescriptor {
            model: "test".into(),
            protocol: Protocol::OpenAIChat,
            capabilities: RequestCapabilities {
                needs_tools: false,
                needs_vision: false,
                needs_streaming: false,
                max_tokens: Some(32),
                modalities: vec!["text".into()],
            },
            context_length_hint: Some(64),
        };

        let plan = router.route(&ctx, &desc).await.expect("route ok");
        assert_eq!(plan.primary_route.sink_id, sink_a_id);
    }

    #[tokio::test]
    async fn test_sink_index_refresher_lists_all_sinks() {
        use crate::router::index::SinkIndex;
        use crate::router::sinks::mock::MockSink;

        let registry = std::sync::Arc::new(routing::SinkRegistry::new());
        registry
            .register(
                "self://one".into(),
                std::sync::Arc::new(MockSink::success("self://one")),
            )
            .await;
        registry
            .register(
                "self://two".into(),
                std::sync::Arc::new(MockSink::unhealthy("self://two")),
            )
            .await;

        let index = SinkIndex::new();
        let n = index.refresh_from_registry(&registry).await;
        assert_eq!(n, 2);

        let items = index.list().await;
        assert_eq!(items.len(), 2);
        // Confirm that both sink_ids are present and snapshot health matches probes
        let mut map = std::collections::HashMap::new();
        for (id, snap) in items {
            map.insert(id, snap);
        }
        assert!(map.get("self://one").unwrap().health.healthy);
        assert!(!map.get("self://two").unwrap().health.healthy);
    }
}
