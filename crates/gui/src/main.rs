#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

#[macro_use]
extern crate tracing;

mod commands;

use gate_core::tracing::{
    config::{InstrumentationConfig, OtlpConfig},
    init::init_tracing,
};
use gate_daemon::Daemon;
use tauri::Manager;

fn main() {
    // Initialize rustls crypto provider for TLS connections
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Initialize instrumentation
    let instrumentation_config = InstrumentationConfig {
        service_name: "gate-gui".to_string(),
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
    init_tracing(&instrumentation_config).expect("Failed to initialize tracing");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::start_daemon,
            commands::stop_daemon,
            commands::daemon_status,
            commands::get_daemon_config,
            commands::restart_daemon,
            commands::get_daemon_status,
            commands::get_daemon_runtime_config,
            commands::get_tlsforward_status,
            commands::configure_tlsforward,
            commands::enable_tlsforward,
            commands::disable_tlsforward,
            commands::get_bootstrap_url,
            commands::get_bootstrap_token,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Get the app handle from the window
                let app = window.app_handle();
                if let Some(daemon) = app.try_state::<Option<Daemon>>() {
                    if daemon.is_some() {
                        tracing::info!("Stopping daemon on window close");
                        let _ = tauri::async_runtime::block_on(commands::stop_daemon(daemon));
                    } else {
                        tracing::warn!("No daemon instance found on window close");
                    }
                }
            }
        })
        .setup(|app| {
            // Optionally start the daemon automatically on app launch
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // // Wait a moment for the app to fully initialize
                match commands::start_daemon(handle.clone()).await {
                    Ok(msg) => tracing::info!("{}", msg),
                    Err(e) => tracing::error!("Failed to auto-start daemon: {}", e),
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
