#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod state;

use state::DaemonState;
use tauri::Manager;

fn main() {
    // Initialize rustls crypto provider for TLS connections
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Initialize tracing for the GUI app
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "".to_string()))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(DaemonState::new())
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
                if let Some(state) = app.try_state::<DaemonState>() {
                    let _ = tauri::async_runtime::block_on(commands::stop_daemon(state));
                }
            }
        })
        .setup(|app| {
            // Optionally start the daemon automatically on app launch
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Wait a moment for the app to fully initialize
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                if let Some(state) = handle.try_state::<DaemonState>() {
                    // Auto-start daemon with default config
                    match commands::start_daemon(state, handle.clone(), None).await {
                        Ok(msg) => tracing::info!("{}", msg),
                        Err(e) => tracing::error!("Failed to auto-start daemon: {}", e),
                    }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
