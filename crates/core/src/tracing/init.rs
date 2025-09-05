//! Initialization functions for tracing
//!
//! This module provides functions to initialize the tracing subsystem
//! with optional OpenTelemetry export.

use anyhow::Result;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::tracing::config::InstrumentationConfig;

/// Initialize tracing with the given configuration
pub fn init_tracing(config: &InstrumentationConfig) -> Result<()> {
    // Create env filter
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&config.log_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // Create the base subscriber with formatting layer
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true);

    // Initialize based on whether OTLP is configured
    if let Some(otlp_config) = &config.otlp {
        // Initialize with OTLP export
        init_with_otlp(config, otlp_config, env_filter)?;
    } else {
        // Initialize without OTLP
        tracing_subscriber::registry()
            .with(fmt_layer)
            .with(env_filter)
            .init();
    }

    Ok(())
}

/// Initialize with OTLP export
#[cfg(feature = "tracing-otlp")]
fn init_with_otlp(
    _config: &InstrumentationConfig,
    otlp_config: &crate::tracing::config::OtlpConfig,
    env_filter: EnvFilter,
) -> Result<()> {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::trace::{RandomIdGenerator, Sampler, SdkTracerProvider};
    use tracing_opentelemetry::OpenTelemetryLayer;

    // Note: In this version of OpenTelemetry, resource creation is limited
    // We'll set service info through environment variables instead

    // Create OTLP exporter
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(&otlp_config.endpoint)
        .build()?;

    // Create tracer provider
    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_sampler(Sampler::AlwaysOn)
        .with_id_generator(RandomIdGenerator::default())
        .build();

    // Create OpenTelemetry layer
    let otel_layer = OpenTelemetryLayer::new(tracer_provider.tracer("gate"));

    // Create the formatting layer
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true);

    // Initialize subscriber with all layers
    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(env_filter)
        .with(otel_layer)
        .init();

    Ok(())
}

/// Initialize with OTLP export (stub for when feature is disabled)
#[cfg(not(feature = "tracing-otlp"))]
fn init_with_otlp(
    _config: &InstrumentationConfig,
    _otlp_config: &crate::tracing::config::OtlpConfig,
    env_filter: EnvFilter,
) -> Result<()> {
    // Create the formatting layer
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true);

    // Just initialize without OTLP when feature is disabled
    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(env_filter)
        .init();

    warn!("OTLP export requested but tracing-otlp feature is not enabled");
    Ok(())
}

/// Initialize with default configuration from environment
pub fn init_default() -> Result<()> {
    let config = InstrumentationConfig::from_env();
    init_tracing(&config)
}

/// Initialize with development configuration
pub fn init_dev() -> Result<()> {
    let config = InstrumentationConfig::dev();
    init_tracing(&config)
}

/// Initialize with Jaeger OTLP export
pub fn init_with_jaeger() -> Result<()> {
    let jaeger_endpoint =
        std::env::var("JAEGER_ENDPOINT").unwrap_or_else(|_| "http://localhost:4317".to_string());
    let config = InstrumentationConfig::with_jaeger(jaeger_endpoint);
    init_tracing(&config)
}

/// Shutdown OpenTelemetry providers gracefully
pub fn shutdown_tracer_provider() {
    // Note: In newer versions of OpenTelemetry, shutdown happens automatically
    // when the tracer provider is dropped. We keep this function for API compatibility.
}
