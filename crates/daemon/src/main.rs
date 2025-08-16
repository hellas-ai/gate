#[macro_use]
extern crate tracing;

use anyhow::Result;
use clap::Parser;
use gate_core::tracing::{
    config::{InstrumentationConfig, OtlpConfig},
    init::init_tracing,
};
use gate_daemon::{Daemon, Settings, StateDir};

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

    // Build daemon
    let mut builder = Daemon::builder();
    let state_dir = StateDir::new().await?;
    let default_config_path = state_dir.config_path();

    // Load configuration if specified
    if let Some(config_path) = cli.config {
        info!("Loading configuration from: {}", config_path);
        let settings = Settings::load_from_file(&config_path)?;
        builder = builder.with_settings(settings);
    } else {
        info!(
            "No configuration path specified, using default at {}",
            default_config_path.display()
        );
        if default_config_path.exists() {
            info!(
                "Loading configuration from default path: {}",
                default_config_path.display()
            );
            builder = builder.with_settings(Settings::load_from_file(&default_config_path)?);
        } else {
            info!("No configuration found, creating one using default settings");
            let settings = Settings::default();
            settings.save_to_file(&default_config_path).await?;
            builder = builder.with_settings(settings);
        }
    }

    // Pass state_dir to builder
    builder = builder.with_state_dir(state_dir);

    // Set static directory if specified
    if let Ok(static_dir) = std::env::var("GATE_SERVER__STATIC_DIR") {
        info!("Using static directory from environment: {}", static_dir);
        builder = builder.with_static_dir(static_dir);
    } else {
        builder = builder.with_static_dir("crates/frontend-daemon/dist".to_string());
    }

    // Build the daemon
    let daemon = builder.build().await?;

    // Print startup information
    let bootstrap_manager = daemon.get_bootstrap_manager().await?;
    if let Some(token) = bootstrap_manager.get_token().await {
        println!("\n===========================================");
        println!("First-time setup required!");
        println!("Please visit the following URL to create your admin account:");
        println!(
            "\n  http://localhost:{}/bootstrap/{}",
            daemon
                .server_address()
                .await?
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
        println!("\n  URL: http://{}/", daemon.server_address().await?);
        let user_count = daemon.user_count().await?;
        if user_count == 1 {
            println!("  Users: {user_count} registered user");
        } else {
            println!("  Users: {user_count} registered users");
        }
        println!("\n  Login with your passkey to access the admin panel");
        println!("===========================================\n");
    }

    // Spawn server task
    let daemon_clone = daemon.clone();
    let _server_handle = tokio::spawn(async move {
        if let Err(e) = daemon_clone.serve().await {
            tracing::error!("Server error: {}", e);
        }
    });

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;
    info!("Received shutdown signal");

    // Graceful shutdown
    daemon.system_identity().shutdown().await?;

    Ok(())
}
