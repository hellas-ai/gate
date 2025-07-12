use crate::config::LocalInferenceConfig;
use anyhow;
use async_trait::async_trait;
use gate_core::inference::{
    AnthropicContent, AnthropicMessage, AnthropicMessageRequest, AnthropicMessageResponse,
    AnthropicMessageStream, AnthropicUsage, ChatChoice, ChatCompletionRequest,
    ChatCompletionResponse, ChatCompletionStream, ChatMessage, InferenceBackend, LocalModel, Usage,
};
use gate_core::{Error, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, instrument, warn};

pub struct LocalInferenceService {
    config: LocalInferenceConfig,
    catgrad_worker: Option<Arc<catgrad::CatgradWorker>>,
}

impl LocalInferenceService {
    pub fn new(config: LocalInferenceConfig) -> Result<Self> {
        // Initialize catgrad worker if local inference is enabled
        let catgrad_worker = if config.enabled {
            info!("Initializing catgrad worker for local inference");
            match catgrad::CatgradWorker::new() {
                Ok(worker) => Some(Arc::new(worker)),
                Err(e) => {
                    error!("Failed to initialize catgrad worker: {}", e);
                    None
                }
            }
        } else {
            info!("Local inference is disabled");
            None
        };

        Ok(Self {
            config,
            catgrad_worker,
        })
    }

    fn convert_chat_to_prompt(&self, messages: &[ChatMessage]) -> String {
        let mut prompt = String::new();

        for message in messages {
            match message.role.as_str() {
                "system" => {
                    prompt.push_str(&format!("System: {}\n\n", message.content));
                }
                "user" => {
                    prompt.push_str(&format!("User: {}\n\n", message.content));
                }
                "assistant" => {
                    prompt.push_str(&format!("Assistant: {}\n\n", message.content));
                }
                _ => {
                    warn!("Unknown role: {}", message.role);
                }
            }
        }

        prompt.push_str("Assistant: ");
        prompt
    }

    fn convert_anthropic_to_prompt(
        &self,
        messages: &[AnthropicMessage],
        system: Option<&str>,
    ) -> String {
        let mut prompt = String::new();

        if let Some(system) = system {
            prompt.push_str(&format!("System: {system}\n\n"));
        }

        for message in messages {
            match message.role.as_str() {
                "user" => {
                    prompt.push_str(&format!("User: {}\n\n", message.content));
                }
                "assistant" => {
                    prompt.push_str(&format!("Assistant: {}\n\n", message.content));
                }
                _ => {
                    warn!("Unknown role: {}", message.role);
                }
            }
        }

        prompt.push_str("Assistant: ");
        prompt
    }
}

#[async_trait]
impl InferenceBackend for LocalInferenceService {
    #[instrument(skip(self, request))]
    async fn chat_completions(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        let temperature = request
            .temperature
            .unwrap_or(self.config.default_temperature);
        let max_tokens = request.max_tokens.unwrap_or(self.config.default_max_tokens);

        debug!("Running inference with {} messages", request.messages.len());

        // Use catgrad worker if available, otherwise fall back to mock
        let response_text = if let Some(worker) = &self.catgrad_worker {
            worker
                .generate(
                    request.model.clone(),
                    request.messages.clone(),
                    temperature,
                    max_tokens as usize,
                )
                .await?
        } else {
            let prompt = self.convert_chat_to_prompt(&request.messages);
            format!(
                "This is a mock response from local model '{}'. Temperature: {}, Max tokens: {}. \
                 No catgrad worker available. Prompt preview: '{}'",
                request.model,
                temperature,
                max_tokens,
                prompt.chars().take(100).collect::<String>()
            )
        };

        let usage = Usage {
            prompt_tokens: request
                .messages
                .iter()
                .map(|m| m.content.split_whitespace().count())
                .sum::<usize>() as u32,
            completion_tokens: response_text.split_whitespace().count() as u32,
            total_tokens: 0, // Will be calculated below
        };
        let usage = Usage {
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.prompt_tokens + usage.completion_tokens,
        };

        Ok(ChatCompletionResponse {
            id: format!("local-{}", uuid::Uuid::new_v4()),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: request.model,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: response_text,
                    name: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(usage),
        })
    }

    async fn chat_completions_stream(
        &self,
        _request: ChatCompletionRequest,
    ) -> Result<ChatCompletionStream> {
        // TODO: Implement streaming when we integrate a real inference engine
        Err(Error::Internal(
            "Streaming not yet implemented for local inference".to_string(),
        ))
    }

    async fn messages(&self, request: AnthropicMessageRequest) -> Result<AnthropicMessageResponse> {
        let temperature = request
            .temperature
            .unwrap_or(self.config.default_temperature);
        let max_tokens = request.max_tokens;

        // Convert Anthropic messages to ChatMessage format
        let mut chat_messages = Vec::new();

        // Add system message if present
        if let Some(system) = &request.system {
            chat_messages.push(ChatMessage {
                role: "system".to_string(),
                content: system.clone(),
                name: None,
            });
        }

        // Add conversation messages
        for msg in &request.messages {
            chat_messages.push(ChatMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
                name: None,
            });
        }

        debug!(
            "Running inference with {} messages (Anthropic format)",
            chat_messages.len()
        );

        // Use catgrad worker if available
        let response_text = if let Some(worker) = &self.catgrad_worker {
            worker
                .generate(
                    request.model.clone(),
                    chat_messages.clone(),
                    temperature,
                    max_tokens as usize,
                )
                .await?
        } else {
            let prompt =
                self.convert_anthropic_to_prompt(&request.messages, request.system.as_deref());
            format!(
                "This is a mock response from local model '{}' using Anthropic format. \
                 No catgrad worker available. Temperature: {}, Max tokens: {}. Prompt preview: '{}'",
                request.model,
                temperature,
                max_tokens,
                prompt.chars().take(100).collect::<String>()
            )
        };

        let usage = AnthropicUsage {
            input_tokens: chat_messages
                .iter()
                .map(|m| m.content.split_whitespace().count())
                .sum::<usize>() as u32,
            output_tokens: response_text.split_whitespace().count() as u32,
        };

        Ok(AnthropicMessageResponse {
            id: format!("msg_{}", uuid::Uuid::new_v4()),
            model: request.model,
            role: "assistant".to_string(),
            content: vec![AnthropicContent {
                content_type: "text".to_string(),
                text: response_text,
            }],
            stop_reason: Some("stop_sequence".to_string()),
            stop_sequence: None,
            usage,
        })
    }

    async fn messages_stream(
        &self,
        _request: AnthropicMessageRequest,
    ) -> Result<AnthropicMessageStream> {
        todo!("Streaming not yet implemented for Anthropic messages");
    }

    async fn list_models(&self) -> Result<Vec<LocalModel>> {
        // Return both specific popular models and supported architectures
        let supported_models = vec![
            // Broken
            // GPT-2 models (GPT2LMHeadModel)
            // LocalModel {
            //     id: "openai-community/gpt2".to_string(),
            //     name: "GPT-2 (124M)".to_string(),
            //     provider: "local".to_string(),
            //     path: None,
            //     context_length: 1024,
            //     supports_chat: true,
            //     supports_completion: true,
            // },
            // Llama models (LlamaForCausalLM)
            // LocalModel {
            //     id: "meta-llama/Llama-3.2-1B-Instruct".to_string(),
            //     name: "Llama 3.2 1B Instruct".to_string(),
            //     provider: "local".to_string(),
            //     path: None,
            //     context_length: 128000,
            //     supports_chat: true,
            //     supports_completion: true,
            // },
            // LocalModel {
            //     id: "meta-llama/Llama-3.2-3B-Instruct".to_string(),
            //     name: "Llama 3.2 3B Instruct".to_string(),
            //     provider: "local".to_string(),
            //     path: None,
            //     context_length: 128000,
            //     supports_chat: true,
            //     supports_completion: true,
            // },
            // 2025-07-10T23:29:27.954867Z  INFO gate_daemon::services::inference::catgrad: Loading model: TinyLlama/TinyLlama-1.1B-Chat-v1.0
            // 2025-07-10T23:29:28.569271Z  INFO gate_daemon::services::inference::catgrad: Model TinyLlama/TinyLlama-1.1B-Chat-v1.0 loaded successfully
            // thread '<unnamed>' panicked at /Users/grw/.cargo-gate-smb/git/checkouts/catgrad-8531093d3d852bf9/87f2f47/catgrad-llm/src/run.rs:98:10:
            // template failed to render: Error { kind: InvalidOperation, detail: "tried to use + operator on unsupported types string and undefined", name: "chat", line: 4 }
            // note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
            // LocalModel {
            //     id: "TinyLlama/TinyLlama-1.1B-Chat-v1.0".to_string(),
            //     name: "TinyLlama 1.1B Chat".to_string(),
            //     provider: "local".to_string(),
            //     path: None,
            //     context_length: 2048,
            //     supports_chat: true,
            //     supports_completion: true,
            // },
            // Qwen3 models (Qwen3ForCausalLM)
            LocalModel {
                id: "Qwen/Qwen3-0.6B".to_string(),
                name: "Qwen3 0.6B".to_string(),
                provider: "local".to_string(),
                path: None,
                context_length: 32768,
                supports_chat: true,
                supports_completion: true,
            },
            // Gemma3 models (Gemma3ForCausalLM)
            // LocalModel {
            //     id: "google/gemma-3-1b-it".to_string(),
            //     name: "Gemma 3 1B Instruct".to_string(),
            //     provider: "local".to_string(),
            //     path: None,
            //     context_length: 8192,
            //     supports_chat: true,
            //     supports_completion: true,
            // },
            // Phi3 models (Phi3ForCausalLM)
            // LocalModel {
            //     id: "microsoft/Phi-3-mini-4k-instruct".to_string(),
            //     name: "Phi-3 Mini 4K Instruct".to_string(),
            //     provider: "local".to_string(),
            //     path: None,
            //     context_length: 4096,
            //     supports_chat: true,
            //     supports_completion: true,
            // },
            // segmentation fault
            // LocalModel {
            //     id: "microsoft/Phi-4-mini-instruct".to_string(),
            //     name: "Phi-4 Mini Instruct".to_string(),
            //     provider: "local".to_string(),
            //     path: None,
            //     context_length: 16384,
            //     supports_chat: true,
            //     supports_completion: true,
            // },
            // SmolLM3 models (SmolLM3ForCausalLM)
            // LocalModel {
            //     id: "HuggingFaceTB/SmolLM3-3B-Base".to_string(),
            //     name: "SmolLM3 3B Base".to_string(),
            //     provider: "local".to_string(),
            //     path: None,
            //     context_length: 8192,
            //     supports_chat: true,
            //     supports_completion: true,
            // },
            // // OLMo2 models (Olmo2ForCausalLM)
            // LocalModel {
            //     id: "allenai/OLMo-2-0425-1B-Instruct".to_string(),
            //     name: "OLMo-2 1B Instruct".to_string(),
            //     provider: "local".to_string(),
            //     path: None,
            //     context_length: 4096,
            //     supports_chat: true,
            //     supports_completion: true,
            // },
        ];

        Ok(supported_models)
    }

    async fn get_model(&self, model_id: &str) -> Result<Option<LocalModel>> {
        // TODO: fetch model details from hf/cache
        Ok(Some(LocalModel {
            id: model_id.to_string(),
            name: model_id.to_string(),
            provider: "local".to_string(),
            path: None,
            context_length: 4096,
            supports_chat: true,
            supports_completion: true,
        }))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

pub struct LocalInferenceServiceBuilder {
    config: LocalInferenceConfig,
}

impl LocalInferenceServiceBuilder {
    pub fn new(config: LocalInferenceConfig) -> Self {
        Self { config }
    }

    pub fn build(self) -> anyhow::Result<LocalInferenceService> {
        LocalInferenceService::new(self.config).map_err(|e| anyhow::anyhow!("{}", e))
    }
}

mod catgrad {
    use super::*;
    use catgrad_llm::{
        run::{ModelLoader, ModelRunner, ModelTokenizer},
        serve::{ChatTokenizer, LM, Loader, Message, Tokenizer},
    };
    use std::thread;

    pub struct InferenceRequest {
        pub model: String,
        pub messages: Vec<ChatMessage>,
        pub temperature: f32,
        pub max_tokens: usize,
        pub response_tx: oneshot::Sender<Result<String>>,
    }

    pub struct CatgradWorker {
        request_tx: mpsc::Sender<InferenceRequest>,
    }

    impl CatgradWorker {
        pub fn new() -> Result<Self> {
            let (request_tx, mut request_rx) = mpsc::channel::<InferenceRequest>(32);

            thread::spawn(move || {
                let mut model_cache: HashMap<String, (ModelRunner, ModelTokenizer)> =
                    HashMap::new();

                while let Some(request) = request_rx.blocking_recv() {
                    let response = match model_cache.get_mut(&request.model) {
                        Some((runner, tokenizer)) => {
                            // Use cached model
                            generate_response(
                                runner,
                                tokenizer,
                                request.messages,
                                request.temperature,
                                request.max_tokens,
                            )
                        }
                        None => {
                            // Load new model
                            match load_model(&request.model) {
                                Ok((mut runner, tokenizer)) => {
                                    let response = generate_response(
                                        &mut runner,
                                        &tokenizer,
                                        request.messages,
                                        request.temperature,
                                        request.max_tokens,
                                    );
                                    // Cache the model for future use
                                    model_cache.insert(request.model.clone(), (runner, tokenizer));
                                    response
                                }
                                Err(e) => Err(e),
                            }
                        }
                    };
                    let _ = request.response_tx.send(response);
                }
            });

            Ok(CatgradWorker { request_tx })
        }
    }

    fn load_model(model_name: &str) -> Result<(ModelRunner, ModelTokenizer)> {
        info!("Loading model: {}", model_name);

        let loader = ModelLoader::new(model_name, true)
            .map_err(|e| Error::Internal(format!("Failed to create model loader: {e}")))?;

        let runner = loader
            .load_runner()
            .map_err(|e| Error::Internal(format!("Failed to load model runner: {e}")))?;

        let tokenizer = loader
            .load_tokenizer()
            .map_err(|e| Error::Internal(format!("Failed to load tokenizer: {e}")))?;

        info!("Model {} loaded successfully", model_name);
        Ok((runner, tokenizer))
    }

    impl CatgradWorker {
        pub async fn generate(
            &self,
            model: String,
            messages: Vec<ChatMessage>,
            temperature: f32,
            max_tokens: usize,
        ) -> Result<String> {
            let (response_tx, response_rx) = oneshot::channel();

            let request = InferenceRequest {
                model,
                messages,
                temperature,
                max_tokens,
                response_tx,
            };

            self.request_tx.send(request).await.map_err(|_| {
                Error::Internal("Failed to send request to inference worker".to_string())
            })?;

            response_rx.await.map_err(|_| {
                Error::Internal("Failed to receive response from inference worker".to_string())
            })?
        }
    }

    fn generate_response(
        runner: &mut ModelRunner,
        tokenizer: &ModelTokenizer,
        messages: Vec<ChatMessage>,
        _temperature: f32, // TODO: Implement temperature sampling
        max_tokens: usize,
    ) -> Result<String> {
        // Convert ChatMessage to catgrad Message format
        let catgrad_messages: Vec<Message> = messages
            .iter()
            .map(|msg| Message {
                role: msg.role.clone(),
                content: msg.content.clone(),
            })
            .collect();

        // Encode messages to tokens
        let context = tokenizer
            .encode_messages(catgrad_messages)
            .map_err(|e| Error::Internal(format!("Failed to encode messages: {e}")))?;

        // Generate tokens
        let mut generated_tokens = Vec::new();
        let mut token_count = 0;

        for token in runner.complete(context) {
            generated_tokens.push(token);
            token_count += 1;

            if token_count >= max_tokens {
                break;
            }
        }

        // Decode tokens back to text
        let response = tokenizer
            .decode(generated_tokens)
            .map_err(|e| Error::Internal(format!("Failed to decode tokens: {e}")))?;

        Ok(response)
    }
}
