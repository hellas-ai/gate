//! Request dispatcher for forwarding inference requests to upstreams

use crate::tracing::Instrument;
use crate::{error::HttpError, forwarding::UpstreamRegistry};
use axum::{
    body::Body,
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use gate_core::InferenceBackend;
use gate_core::tracing::{CorrelationId, trace_context::inject_trace_context};
use serde_json::Value as JsonValue;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

/// Dispatcher for routing inference requests to appropriate upstreams
pub struct Dispatcher {
    upstream_registry: Arc<UpstreamRegistry>,
    inference_backend: Option<Arc<dyn InferenceBackend>>,
}

impl Dispatcher {
    /// Create a new dispatcher with the given upstream registry
    pub fn new(upstream_registry: Arc<UpstreamRegistry>) -> Self {
        Self {
            upstream_registry,
            inference_backend: None,
        }
    }

    /// Create a new dispatcher with the given upstream registry and inference backend
    pub fn with_inference_backend(
        upstream_registry: Arc<UpstreamRegistry>,
        inference_backend: Arc<dyn InferenceBackend>,
    ) -> Self {
        Self {
            upstream_registry,
            inference_backend: Some(inference_backend),
        }
    }

    /// Dispatch an Anthropic messages request
    #[instrument(
        name = "dispatch_messages",
        skip(self, headers),
        fields(model = %model)
    )]
    pub async fn messages(
        &self,
        model: &str,
        request: JsonValue,
        headers: HeaderMap,
        correlation_id: CorrelationId,
    ) -> Result<Response, HttpError> {
        let endpoint = "/messages";
        let request_type = "anthropic_messages";

        self.dispatch_request(
            model,
            request,
            headers,
            correlation_id,
            endpoint,
            request_type,
        )
        .await
    }

    /// Dispatch an OpenAI chat completions request
    #[instrument(
        name = "dispatch_chat_completions",
        skip(self, headers),
        fields(model = %model)
    )]
    pub async fn chat_completions(
        &self,
        model: &str,
        request: JsonValue,
        headers: HeaderMap,
        correlation_id: CorrelationId,
    ) -> Result<Response, HttpError> {
        let endpoint = "/chat/completions";
        let request_type = "openai_chat_completions";

        self.dispatch_request(
            model,
            request,
            headers,
            correlation_id,
            endpoint,
            request_type,
        )
        .await
    }

    /// Dispatch an OpenAI responses request
    #[instrument(
        name = "dispatch_responses",
        skip(self, headers),
        fields(model = %model)
    )]
    pub async fn responses(
        &self,
        model: &str,
        request: JsonValue,
        headers: HeaderMap,
        correlation_id: CorrelationId,
    ) -> Result<Response, HttpError> {
        let endpoint = "/responses";
        let request_type = "openai_responses";

        self.dispatch_request(
            model,
            request,
            headers,
            correlation_id,
            endpoint,
            request_type,
        )
        .await
    }

    /// Common logic for dispatching requests
    async fn dispatch_request(
        &self,
        model: &str,
        request: JsonValue,
        headers: HeaderMap,
        correlation_id: CorrelationId,
        endpoint: &str,
        request_type: &str,
    ) -> Result<Response, HttpError> {
        debug!(
            "Request body: {:?}",
            serde_json::to_string(&request).unwrap_or_default()
        );

        // Check if streaming is requested
        let is_streaming = request
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if is_streaming {
            return Err(HttpError::NotImplemented(
                "Streaming not yet implemented".into(),
            ));
        }

        // Check if any upstreams are configured
        if self.upstream_registry.has_upstreams().await {
            // Get the upstream configuration for this model
            let forwarding_config = match self.upstream_registry.get_upstream_for_model(model).await
            {
                Some(config) => {
                    info!("Found upstream for model {} - forwarding request", model);
                    config
                }
                None => {
                    warn!("No upstream configured for model: {}", model);
                    let all_upstreams = self.upstream_registry.get_all_upstreams().await;
                    for (name, info) in all_upstreams {
                        debug!("Upstream '{}' has models: {:?}", name, info.models);
                    }
                    return Err(HttpError::BadRequest(format!(
                        "No upstream configured for model: {model}"
                    )));
                }
            };

            // Forward the request to the upstream provider
            let client = Self::create_http_client(forwarding_config.timeout_seconds)?;

            let url = format!("{}{}", forwarding_config.base_url, endpoint);
            let mut req = client.post(&url).json(&request);

            // Add authentication header
            if let Some((header_name, header_value)) = forwarding_config.auth_header() {
                req = req.header(header_name, header_value);
            }

            // Add provider-specific headers
            for (name, value) in forwarding_config.provider_headers() {
                req = req.header(name, value);
            }

            // Forward relevant headers from the original request
            for (name, value) in headers.iter() {
                let name_str = name.as_str();
                // Skip certain headers that shouldn't be forwarded
                if !matches!(
                    name_str,
                    "host" | "content-length" | "content-type" | "authorization" | "x-api-key"
                ) {
                    req = req.header(name.clone(), value.clone());
                }
            }

            // Inject W3C trace context headers
            let mut trace_headers = HeaderMap::new();
            if let Err(e) = inject_trace_context(correlation_id.trace_context(), &mut trace_headers)
            {
                warn!("Failed to inject trace context headers: {}", e);
            }
            for (name, value) in trace_headers.iter() {
                req = req.header(name.clone(), value.clone());
            }

            let response = {
                let span = info_span!(
                    "upstream_request",
                    url = %url,
                    provider = %forwarding_config.provider,
                    model = %model,
                    method = "POST",
                    endpoint = %endpoint,
                    timeout_seconds = forwarding_config.timeout_seconds,
                    stream = is_streaming,
                    request_type = %request_type
                );
                req.send().instrument(span).await.map_err(|e| {
                    HttpError::ServiceUnavailable(format!("Failed to forward request: {e}"))
                })?
            };

            let status = response.status();
            let headers = response.headers().clone();

            // Check if the response is an error (only for chat completions endpoint)
            if !status.is_success() && endpoint == "/chat/completions" {
                let error_body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unable to read error response".to_string());
                warn!("Upstream returned error status {}: {}", status, error_body);

                // Try to parse as JSON to get better error details
                if let Ok(error_json) = serde_json::from_str::<JsonValue>(&error_body) {
                    return Err(HttpError::InternalServerError(format!(
                        "Upstream error: {status} - {error_json}"
                    )));
                }

                return Err(HttpError::InternalServerError(format!(
                    "Upstream error: {status} - {error_body}"
                )));
            }

            let body = response.json::<JsonValue>().await.map_err(|e| {
                HttpError::InternalServerError(format!("Failed to parse response: {e}"))
            })?;

            let mut response_builder = axum::http::Response::builder().status(status.as_u16());

            // Forward relevant headers from upstream response
            for (name, value) in headers.iter() {
                let name_str = name.as_str();
                // Skip headers that Axum will set automatically
                if !matches!(
                    name_str,
                    "content-length" | "transfer-encoding" | "connection"
                ) {
                    response_builder = response_builder.header(name, value);
                }
            }

            response_builder
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .map_err(|e| {
                    HttpError::InternalServerError(format!("Failed to build response: {e}"))
                })
        } else {
            // No upstreams configured, fallback to local inference
            if let Some(inference_backend) = &self.inference_backend {
                // Check if the model is available locally
                let model_info = inference_backend.get_model(model).await.map_err(|e| {
                    HttpError::InternalServerError(format!("Failed to get model info: {e}"))
                })?;

                if model_info.is_none() {
                    return Err(HttpError::BadRequest(format!(
                        "Model '{model}' is not available locally"
                    )));
                }

                // Dispatch based on the endpoint
                match endpoint {
                    "/chat/completions" => {
                        self.handle_local_chat_completion(inference_backend.clone(), request)
                            .await
                    }
                    "/messages" => {
                        self.handle_local_messages(inference_backend.clone(), request)
                            .await
                    }
                    _ => Err(HttpError::NotImplemented(format!(
                        "Local inference not implemented for endpoint: {endpoint}"
                    ))),
                }
            } else {
                Err(HttpError::NotImplemented(
                    "No inference backend configured".to_string(),
                ))
            }
        }
    }

    /// Handle local chat completion request
    async fn handle_local_chat_completion(
        &self,
        inference_backend: Arc<dyn InferenceBackend>,
        request: JsonValue,
    ) -> Result<Response, HttpError> {
        // Parse the request
        let chat_request: gate_core::inference::ChatCompletionRequest =
            serde_json::from_value(request).map_err(|e| {
                HttpError::BadRequest(format!("Invalid chat completion request: {e}"))
            })?;

        // Check if streaming is requested
        if chat_request.stream.unwrap_or(false) {
            // Handle streaming response
            let stream = inference_backend
                .chat_completions_stream(chat_request)
                .await
                .map_err(|e| {
                    HttpError::InternalServerError(format!(
                        "Failed to generate streaming response: {e}"
                    ))
                })?;

            // Convert stream to SSE format
            use axum::response::sse::{Event, Sse};
            use futures::StreamExt;

            let sse_stream = stream.map(|result| {
                result
                    .map(|chunk| {
                        Event::default().data(serde_json::to_string(&chunk).unwrap_or_default())
                    })
                    .map_err(|e| {
                        axum::Error::new(std::io::Error::other(format!("Stream error: {e}")))
                    })
            });

            let sse = Sse::new(sse_stream).keep_alive(axum::response::sse::KeepAlive::default());

            Ok(sse.into_response())
        } else {
            // Handle non-streaming response
            let response = inference_backend
                .chat_completions(chat_request)
                .await
                .map_err(|e| {
                    HttpError::InternalServerError(format!("Failed to generate response: {e}"))
                })?;

            let body = serde_json::to_vec(&response).map_err(|e| {
                HttpError::InternalServerError(format!("Failed to serialize response: {e}"))
            })?;

            axum::http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .map_err(|e| {
                    HttpError::InternalServerError(format!("Failed to build response: {e}"))
                })
        }
    }

    /// Handle local messages request (Anthropic format)
    async fn handle_local_messages(
        &self,
        inference_backend: Arc<dyn InferenceBackend>,
        request: JsonValue,
    ) -> Result<Response, HttpError> {
        // Parse the request
        let messages_request: gate_core::inference::AnthropicMessageRequest =
            serde_json::from_value(request)
                .map_err(|e| HttpError::BadRequest(format!("Invalid messages request: {e}")))?;

        // Check if streaming is requested
        if messages_request.stream.unwrap_or(false) {
            // Handle streaming response
            let stream = inference_backend
                .messages_stream(messages_request)
                .await
                .map_err(|e| {
                    HttpError::InternalServerError(format!(
                        "Failed to generate streaming response: {e}"
                    ))
                })?;

            // Convert stream to SSE format
            use axum::response::sse::{Event, Sse};
            use futures::StreamExt;

            let sse_stream = stream.map(|result| {
                result
                    .map(|chunk| {
                        Event::default().data(serde_json::to_string(&chunk).unwrap_or_default())
                    })
                    .map_err(|e| {
                        axum::Error::new(std::io::Error::other(format!("Stream error: {e}")))
                    })
            });

            let sse = Sse::new(sse_stream).keep_alive(axum::response::sse::KeepAlive::default());

            Ok(sse.into_response())
        } else {
            // Handle non-streaming response
            let response = inference_backend
                .messages(messages_request)
                .await
                .map_err(|e| {
                    HttpError::InternalServerError(format!("Failed to generate response: {e}"))
                })?;

            let body = serde_json::to_vec(&response).map_err(|e| {
                HttpError::InternalServerError(format!("Failed to serialize response: {e}"))
            })?;

            axum::http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .map_err(|e| {
                    HttpError::InternalServerError(format!("Failed to build response: {e}"))
                })
        }
    }

    /// Create an HTTP client with optional timeout based on target architecture
    #[allow(unused_variables)]
    fn create_http_client(timeout_seconds: u64) -> Result<reqwest::Client, HttpError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout_seconds))
                .build()
                .map_err(|e| {
                    HttpError::InternalServerError(format!("Failed to create HTTP client: {e}"))
                })
        }

        #[cfg(target_arch = "wasm32")]
        {
            // WASM doesn't support timeout on Client::builder()
            reqwest::Client::builder().build().map_err(|e| {
                HttpError::InternalServerError(format!("Failed to create HTTP client: {e}"))
            })
        }
    }
}
