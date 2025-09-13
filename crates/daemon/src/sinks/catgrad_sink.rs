use async_trait::async_trait;
use catgrad_llm::serve::Loader;
use futures::StreamExt;
use gate_core::Result;
use gate_core::router::prelude::{
    Protocol, RequestContext, RequestStream, ResponseChunk, Sink, SinkCapabilities,
    SinkDescription, SinkHealth, StopReason,
};
use serde_json::json;
use std::pin::Pin;

use catgrad_llm::{
    run::{ModelLoader, ModelRunner, ModelTokenizer},
    serve::{ChatTokenizer, LM, Message, Tokenizer},
};

pub struct CatgradSink {
    id: String,
}

impl CatgradSink {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[async_trait]
impl Sink for CatgradSink {
    async fn describe(&self) -> SinkDescription {
        SinkDescription {
            id: self.id.clone(),
            accepted_protocols: vec![Protocol::OpenAIChat, Protocol::Anthropic],
            capabilities: SinkCapabilities {
                supports_streaming: true,
                supports_batching: false,
                supports_tools: false,
                max_context_length: Some(8192),
                modalities: vec!["text".into()],
            },
            cost_structure: None,
        }
    }

    async fn probe(&self) -> SinkHealth {
        SinkHealth {
            healthy: true,
            latency_ms: Some(10),
            error_rate: 0.0,
            last_error: None,
            last_check: chrono::Utc::now(),
        }
    }

    async fn execute(
        &self,
        _ctx: &RequestContext,
        mut request: RequestStream,
    ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<ResponseChunk>> + Send>>> {
        let protocol = request.protocol();
        #[inline]
        fn internalize<E: std::fmt::Display>(e: E) -> gate_core::Error {
            gate_core::Error::Internal(format!("{e}"))
        }
        // Take first request JSON
        let first = request.next().await.ok_or_else(|| {
            gate_core::Error::InvalidRoutingConfig("Empty request stream".to_string())
        })??;

        let model = first
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Build messages in a simple, provider-agnostic way
        let mut catgrad_messages: Vec<Message> = Vec::new();
        if let Some(system) = first.get("system").and_then(|v| v.as_str()) {
            catgrad_messages.push(Message {
                role: "system".into(),
                content: system.into(),
            });
        }

        if let Some(msgs) = first.get("messages").and_then(|v| v.as_array()) {
            for m in msgs {
                let role = m
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("user")
                    .to_string();
                // Handle string content or anthropic-style content blocks
                let content = if let Some(s) = m.get("content").and_then(|v| v.as_str()) {
                    s.to_string()
                } else if let Some(arr) = m.get("content").and_then(|v| v.as_array()) {
                    let mut s = String::new();
                    for block in arr {
                        if let Some(t) = block.get("type").and_then(|v| v.as_str())
                            && t == "text"
                            && let Some(txt) = block.get("text").and_then(|v| v.as_str())
                        {
                            s.push_str(txt);
                            s.push('\n');
                        }
                    }
                    s
                } else {
                    String::new()
                };
                catgrad_messages.push(Message { role, content });
            }
        }

        let _temperature = first
            .get("temperature")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.7) as f32;
        let max_tokens = first
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(256) as usize;

        // Spawn blocking generation to compute deltas of decoded text and usage
        let model_clone = model.clone();
        let (deltas, prompt_tokens, completion_tokens): (Vec<String>, u32, u32) =
            tokio::task::spawn_blocking(move || -> gate_core::Result<(Vec<String>, u32, u32)> {
                let loader = ModelLoader::new(&model_clone, true).map_err(internalize)?;
                let mut runner: ModelRunner = loader.load_runner().map_err(internalize)?;
                let tokenizer: ModelTokenizer = loader.load_tokenizer().map_err(internalize)?;

                let context = tokenizer
                    .encode_messages(catgrad_messages)
                    .map_err(internalize)?;
                let prompt_len = context.len() as u32;

                let mut deltas = Vec::new();
                let mut count = 0usize;
                for token in runner.complete(context) {
                    // Decode only the current token to avoid cloning the full sequence
                    if let Ok(piece) = tokenizer.decode(vec![token])
                        && !piece.is_empty()
                    {
                        deltas.push(piece);
                    }
                    count += 1;
                    if count >= max_tokens {
                        break;
                    }
                }
                Ok((deltas, prompt_len, count as u32))
            })
            .await
            .map_err(internalize)??;

        // Build a stream over headers, metadata, incremental deltas, usage, then Stop
        let mut items: Vec<gate_core::Result<ResponseChunk>> = Vec::with_capacity(deltas.len() + 4);
        items.push(Ok(ResponseChunk::Headers(Default::default())));
        items.push(Ok(ResponseChunk::Metadata({
            let mut m = std::collections::HashMap::new();
            m.insert("provider".to_string(), json!("local"));
            m.insert("model".to_string(), json!(model));
            m
        })));
        for d in deltas {
            let chunk_body = match protocol {
                Protocol::OpenAIChat => json!({
                    "choices": [{"index": 0, "delta": {"content": d}}]
                }),
                Protocol::Anthropic => json!({
                    "content": [{"type": "text", "text": d}]
                }),
                _ => json!({"delta": d}),
            };
            items.push(Ok(ResponseChunk::Content(chunk_body)));
        }
        items.push(Ok(ResponseChunk::Usage {
            prompt_tokens,
            completion_tokens,
        }));
        items.push(Ok(ResponseChunk::Stop {
            reason: StopReason::Complete,
            error: None,
            cost: None,
        }));
        Ok(Box::pin(futures::stream::iter(items)))
    }
}
