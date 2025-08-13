use anyhow::Result;
use clap::Parser;
use gate_core::tracing::{
    config::{InstrumentationConfig, OtlpConfig},
    init::init_tracing,
};
use gate_daemon::{Settings, runtime::Runtime};
use tracing::info;

/// Gate daemon - High-performance AI gateway
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Configuration file path
    #[arg(short = 'c', long = "config")]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize rustls crypto provider for TLS connections
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Parse command line arguments
    let cli = Cli::parse();

    // Load environment variables from .env file
    dotenvy::dotenv().ok();

    // Initialize instrumentation
    let instrumentation_config = InstrumentationConfig {
        service_name: "gate-daemon".to_string(),
        service_version: env!("CARGO_PKG_VERSION").to_string(),
        log_level: std::env::var("RUST_LOG")
            .unwrap_or_else(|_| "gate=debug,tower_http=debug".to_string()),
        otlp: std::env::var("OTLP_ENDPOINT")
            .ok()
            .map(|endpoint| OtlpConfig {
                endpoint,
                headers: None,
            }),
    };
    init_tracing(&instrumentation_config)?;

    // Build runtime
    let mut builder = Runtime::builder();

    // Load configuration if specified
    if let Some(config_path) = cli.config {
        info!("Loading configuration from: {}", config_path);
        let settings = Settings::load_from_file(&config_path)?;
        builder = builder.with_settings(settings);
    }

    // Set static directory if specified
    if let Ok(static_dir) = std::env::var("GATE_SERVER__STATIC_DIR") {
        builder = builder.with_static_dir(static_dir);
    }

    // Build the runtime
    let runtime = builder.build().await?;

    // Print startup information
    if let Some(token) = runtime.bootstrap_token() {
        println!("\n===========================================");
        println!("First-time setup required!");
        println!("Please visit the following URL to create your admin account:");
        println!(
            "\n  http://localhost:{}/bootstrap/{}",
            runtime
                .server_address()
                .split(':')
                .nth(1)
                .unwrap_or("31145"),
            token
        );
        println!("\n===========================================\n");
        info!("Bootstrap URL printed for token: {}", token);
    } else {
        println!("\n===========================================");
        println!("Gate daemon is running");
        println!("\n  URL: http://{}/", runtime.server_address());
        let user_count = runtime.user_count();
        if user_count == 1 {
            println!("  Users: {user_count} registered user");
        } else {
            println!("  Users: {user_count} registered users");
        }
        println!("\n  Login with your passkey to access the admin panel");
        println!("===========================================\n");
    }

    // Start monitoring tasks
    runtime.start_monitoring().await;

    // Start metrics server
    let metrics_handle = runtime.start_metrics().await?;

    // Spawn server task
    let runtime_clone = runtime.clone();
    let server_handle = tokio::spawn(async move {
        if let Err(e) = runtime_clone.serve().await {
            tracing::error!("Server error: {}", e);
        }
    });

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;
    info!("Received shutdown signal");

    // Graceful shutdown
    runtime.shutdown().await;

    // Wait for server to stop
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), server_handle).await;

    // Stop metrics server
    if let Some(handle) = metrics_handle {
        handle.abort();
    }

    Ok(())
}
