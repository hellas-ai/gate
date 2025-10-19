//! Monitoring middleware for observability

use super::{Middleware, Next, RequestStream, ResponseStream};
use crate::Result;
use crate::router::connector::RequestContext;
use crate::router::types::ResponseChunk;
use async_trait::async_trait;
use futures::StreamExt;
use std::time::Instant;

/// Monitoring middleware for metrics and tracing
pub struct MonitoringMiddleware {
    service_name: String,
}

impl MonitoringMiddleware {
    /// Create a new monitoring middleware
    pub fn new(service_name: String) -> Self {
        Self { service_name }
    }
}

impl Default for MonitoringMiddleware {
    fn default() -> Self {
        Self::new("gate-router".to_string())
    }
}

#[async_trait]
impl Middleware for MonitoringMiddleware {
    async fn process(
        &self,
        ctx: &mut RequestContext,
        request: RequestStream,
        next: Next,
    ) -> Result<ResponseStream> {
        let start_time = Instant::now();
        let trace_id = ctx
            .trace_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());

        // Log request start
        #[cfg(feature = "tracing")]
        {
            info!(
                service = %self.service_name,
                trace_id = %trace_id,
                user_id = ?ctx.identity.context.user_id,
                "Processing request"
            );
        }

        // Process the request
        let result = next(request).await;

        match &result {
            Ok(_) => {
                let duration = start_time.elapsed();
                #[cfg(feature = "tracing")]
                {
                    info!(
                        service = %self.service_name,
                        trace_id = %trace_id,
                        duration_ms = %duration.as_millis(),
                        "Request completed successfully"
                    );
                }

                // Wrap the stream to monitor chunks
                let mut stream = result?;
                let service_name = self.service_name.clone();
                let trace_id_clone = trace_id.clone();

                let monitored_stream = async_stream::stream! {
                    let mut chunk_count = 0;
                    let mut total_tokens = 0u32;
                    let mut has_error = false;

                    while let Some(chunk_result) = stream.next().await {
                        match chunk_result {
                            Ok(chunk) => {
                                chunk_count += 1;

                                // Track token usage
                                if let ResponseChunk::Usage { prompt_tokens, completion_tokens } = &chunk {
                                    total_tokens = prompt_tokens + completion_tokens;
                                    #[cfg(feature = "tracing")]
                                    {
                                        debug!(
                                            service = %service_name,
                                            trace_id = %trace_id_clone,
                                            prompt_tokens = %prompt_tokens,
                                            completion_tokens = %completion_tokens,
                                            "Token usage"
                                        );
                                    }
                                }

                                // Track errors
                                if let ResponseChunk::Stop { error: Some(err), .. } = &chunk {
                                    has_error = true;
                                    #[cfg(feature = "tracing")]
                                    {
                                        error!(
                                            service = %service_name,
                                            trace_id = %trace_id_clone,
                                            error = %err,
                                            "Stream error"
                                        );
                                    }
                                }

                                yield Ok(chunk);
                            }
                            Err(e) => {
                                has_error = true;
                                #[cfg(feature = "tracing")]
                                {
                                    error!(
                                        service = %service_name,
                                        trace_id = %trace_id_clone,
                                        error = %e,
                                        "Stream processing error"
                                    );
                                }
                                yield Err(e);
                            }
                        }
                    }

                    // Log stream completion
                    let total_duration = start_time.elapsed();
                    #[cfg(feature = "tracing")]
                    {
                        info!(
                            service = %service_name,
                            trace_id = %trace_id_clone,
                            chunks = %chunk_count,
                            total_tokens = %total_tokens,
                            duration_ms = %total_duration.as_millis(),
                            success = %!has_error,
                            "Stream completed"
                        );
                    }
                };

                Ok(Box::pin(monitored_stream))
            }
            Err(e) => {
                let duration = start_time.elapsed();
                #[cfg(feature = "tracing")]
                {
                    error!(
                        service = %self.service_name,
                        trace_id = %trace_id,
                        duration_ms = %duration.as_millis(),
                        error = %e,
                        "Request failed"
                    );
                }
                result
            }
        }
    }
}
