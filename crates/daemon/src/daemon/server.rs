//! Server-related functionality extracted from daemon/mod.rs

use crate::{
    State,
    config::{LocalInferenceConfig, ProviderConfig, ProviderType, Settings},
    daemon::{Daemon, Result},
    error::DaemonError,
    services::{LocalInferenceService, key_capture::DaemonKeyRegistrar},
    sinks::catgrad_sink::CatgradSink,
};
use axum::http::HeaderName;
use gate_core::{
    router::{
        Sink,
        index::SinkIndex,
        middleware::KeyCaptureMiddleware,
        registry::SinkRegistry,
        routing::Router,
        strategy::{CompositeStrategy, ProviderAffinityStrategy, SimpleStrategy},
    },
    state::StateBackend,
};
use gate_http::{
    AppState,
    sinks::{
        anthropic::{self, AnthropicConfig},
        openai::{self, OpenAIConfig},
    },
};
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};
use tracing::{info, warn};

pub struct ServerBuilder {
    daemon: Daemon,
    settings: Arc<Settings>,
}

impl ServerBuilder {
    pub fn new(daemon: Daemon, settings: Arc<Settings>) -> Self {
        Self { daemon, settings }
    }

    /// Build and bind TCP listener
    pub async fn bind_listener(&self) -> Result<tokio::net::TcpListener> {
        let addr = format!(
            "{}:{}",
            self.settings.server.host, self.settings.server.port
        );
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(DaemonError::Io)?;
        info!("Server will listen on http://{}", addr);
        Ok(listener)
    }

    /// Initialize base router with authentication routes
    ///
    /// Returns a router that is missing `AppState<State>`.
    /// The actual state value is supplied at the end before serving.
    pub fn init_router(&self) -> axum::Router<AppState<State>> {
        let router: axum::Router<AppState<State>> = axum::Router::new();
        let router = crate::routes::auth::add_routes(router);
        let router = crate::routes::config::add_routes(router);
        crate::routes::admin::add_routes(router)
    }

    /// Create state for the application
    pub async fn create_state(&self) -> Result<State> {
        let auth_service = self.daemon.get_auth_service().await?;
        let allow_local_bypass =
            self.settings.server.allow_local_bypass && is_local_host(&self.settings.server.host);

        Ok(State::new(
            auth_service,
            self.daemon.clone(),
            allow_local_bypass,
            self.settings.auth.provider_passthrough.clone(),
        ))
    }

    /// Register all provider sinks
    pub async fn register_sinks(&self, registry: &Arc<SinkRegistry>) -> Result<()> {
        let mut has_anthropic = false;
        let mut has_openai = false;

        // Register configured provider sinks
        for provider_config in &self.settings.providers {
            let sink = match self.create_provider_sink(provider_config).await {
                Ok(s) => s,
                Err(e) => {
                    warn!("Failed to create {} sink: {}", provider_config.provider, e);
                    continue;
                }
            };

            match provider_config.provider {
                ProviderType::Anthropic => has_anthropic = true,
                ProviderType::OpenAI => has_openai = true,
                ProviderType::Custom => {}
            }

            let sink_id = format_provider_sink_id(&provider_config.provider, &provider_config.name);
            registry.register(sink_id, sink).await;
            info!("Registered provider sink: {}", provider_config.name);
        }

        // Register fallback sinks if needed
        if !has_anthropic {
            self.register_anthropic_fallback(registry).await;
        }
        if !has_openai {
            self.register_openai_fallbacks(registry).await;
        }

        // Register local inference sink if configured
        if let Some(ref inf_cfg) = self.settings.local_inference {
            self.register_catgrad_sink(registry, inf_cfg).await;
        }

        Ok(())
    }

    /// Create a provider sink based on configuration
    async fn create_provider_sink(&self, config: &ProviderConfig) -> Result<Arc<dyn Sink>> {
        match config.provider {
            ProviderType::Anthropic => {
                let anthropic_config = AnthropicConfig {
                    api_key: config.api_key.clone(),
                    base_url: Some(config.base_url.clone()),
                    timeout_seconds: Some(config.timeout_seconds),
                    sink_id: Some(format!("provider://anthropic/{}", config.name)),
                };
                anthropic::create_sink(anthropic_config)
                    .await
                    .map(|sink| Arc::new(sink) as Arc<dyn Sink>)
                    .map_err(|e| DaemonError::ServiceUnavailable(e.to_string()))
            }
            ProviderType::OpenAI => {
                let openai_config = OpenAIConfig {
                    api_key: config.api_key.clone(),
                    base_url: Some(config.base_url.clone()),
                    models: if config.models.is_empty() {
                        None
                    } else {
                        Some(config.models.clone())
                    },
                    timeout_seconds: Some(config.timeout_seconds),
                    sink_id: Some(format!("provider://openai/{}", config.name)),
                };
                openai::create_sink(openai_config)
                    .map(|sink| Arc::new(sink) as Arc<dyn Sink>)
                    .map_err(|e| DaemonError::ServiceUnavailable(e.to_string()))
            }
            ProviderType::Custom => Err(DaemonError::ConfigError(
                "Custom provider type not yet implemented".to_string(),
            )),
        }
    }

    /// Register Anthropic fallback sink
    async fn register_anthropic_fallback(&self, registry: &Arc<SinkRegistry>) {
        match anthropic::create_fallback_sink().await {
            Ok(sink) => {
                registry
                    .register("provider://anthropic/fallback".to_string(), Arc::new(sink))
                    .await;
                info!(
                    "Registered fallback Anthropic sink (no API key; will capture on first success)"
                );
            }
            Err(e) => warn!("Failed to create fallback Anthropic sink: {}", e),
        }
    }

    /// Register OpenAI fallback sinks
    async fn register_openai_fallbacks(&self, registry: &Arc<SinkRegistry>) {
        // Standard OpenAI fallback
        match openai::create_fallback_sink() {
            Ok(sink) => {
                registry
                    .register("provider://openai/fallback".to_string(), Arc::new(sink))
                    .await;
                info!("Registered fallback OpenAI sink (no API key; will accept client keys)");
            }
            Err(e) => warn!("Failed to create fallback OpenAI sink: {}", e),
        }

        // Codex fallback for ChatGPT backend
        match openai::create_codex_fallback_sink() {
            Ok(sink) => {
                registry
                    .register("provider://openai/codex".to_string(), Arc::new(sink))
                    .await;
                info!("Registered fallback OpenAI Codex sink (OAuth tokens; /backend-api/codex)");
            }
            Err(e) => warn!("Failed to create fallback OpenAI Codex sink: {}", e),
        }
    }

    /// Register Catgrad sink for local inference
    async fn register_catgrad_sink(
        &self,
        registry: &Arc<SinkRegistry>,
        config: &LocalInferenceConfig,
    ) {
        match LocalInferenceService::new(config.clone()) {
            Ok(_) => {
                let sink = Arc::new(CatgradSink::new("self://catgrad", config.models.clone()));
                registry.register("self://catgrad".to_string(), sink).await;
                info!("Registered Catgrad sink for local inference");
            }
            Err(e) => warn!("Failed to initialize LocalInferenceService: {}", e),
        }
    }

    /// Build the core router with strategies and middleware
    pub async fn build_router_core(
        &self,
        state_backend: Arc<dyn StateBackend>,
        sink_registry: Arc<SinkRegistry>,
        sink_index: Arc<SinkIndex>,
    ) -> Arc<Router> {
        let registrar = Arc::new(DaemonKeyRegistrar::new(
            self.daemon.clone(),
            sink_registry.clone(),
            sink_index.clone(),
        ));

        let router = Router::builder()
            .state_backend(state_backend)
            .sink_registry(sink_registry)
            .strategy(Box::new(CompositeStrategy::new(vec![
                (Box::new(ProviderAffinityStrategy::new()), 1.0),
                (Box::new(SimpleStrategy::new()), 0.1),
            ])))
            .middleware(Arc::new(KeyCaptureMiddleware::new(registrar)))
            .sink_index(sink_index)
            .build();

        Arc::new(router)
    }

    /// Configure middleware layers
    pub fn configure_middleware<S>(&self, app: axum::Router<S>) -> axum::Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        app.layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::Any)
                .allow_methods(tower_http::cors::Any)
                .allow_headers(vec![
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::AUTHORIZATION,
                    HeaderName::from_static("x-correlation-id"),
                    HeaderName::from_static("x-api-key"),
                    HeaderName::from_static("traceparent"),
                    HeaderName::from_static("tracestate"),
                ])
                .expose_headers(vec![
                    HeaderName::from_static("x-correlation-id"),
                    HeaderName::from_static("traceparent"),
                    HeaderName::from_static("tracestate"),
                ]),
        )
        .layer(axum::middleware::from_fn(
            gate_http::middleware::correlation_id_middleware,
        ))
    }

    /// Add static file serving if configured
    pub fn add_static_serving<S>(&self, app: axum::Router<S>) -> axum::Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        if let Some(static_dir) = &self.daemon.static_dir {
            if std::path::Path::new(static_dir).exists() {
                info!("Serving static files from: {}", static_dir);
                let index_path = format!("{static_dir}/index.html");
                let serve_dir = ServeDir::new(static_dir).fallback(ServeFile::new(index_path));
                return app.fallback_service(serve_dir);
            } else {
                warn!("Static directory not found: {}", static_dir);
            }
        }
        app
    }

    /// Build the complete application
    pub async fn build_app(
        &self,
        router: axum::Router<AppState<State>>,
        app_state: AppState<State>,
    ) -> axum::Router<AppState<State>> {
        let app: axum::Router<AppState<State>> = router;

        let app = app
            // Merge common HTTP routes (health, inference, models, observability)
            .merge(gate_http::routes::router::<State>())
            // Apply auth middleware
            .route_layer(axum::middleware::from_fn_with_state(
                app_state.clone(),
                gate_http::middleware::auth::auth_middleware::<State>,
            ));

        let app = self.configure_middleware(app);
        let app = self.add_static_serving(app);
        gate_http::middleware::with_request_tracing(app)
    }
}

/// Helper function to check if host is localhost
fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// Format provider sink ID
fn format_provider_sink_id(provider: &ProviderType, name: &str) -> String {
    match provider {
        ProviderType::Anthropic => format!("provider://anthropic/{name}"),
        ProviderType::OpenAI => format!("provider://openai/{name}"),
        ProviderType::Custom => format!("provider://{name}"),
    }
}
