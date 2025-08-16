use gate_daemon::types::DaemonRuntimeConfigResponse;
use gate_daemon::{Daemon, DaemonStatus, Settings, StateDir};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub async fn start_daemon(app: AppHandle) -> Result<String, String> {
    // Build daemon
    let state_dir = StateDir::new()
        .await
        .map_err(|e| format!("Failed to create state directory: {e}"))?;
    let default_config_path = state_dir.config_path();
    let mut builder = Daemon::builder().with_state_dir(state_dir);

    // Load configuration if specified
    if default_config_path.exists() {
        info!(
            "Loading configuration from default path: {}",
            default_config_path.display()
        );
        builder = builder.with_settings(
            Settings::load_from_file(&default_config_path)
                .map_err(|e| format!("Failed to load settings: {e}"))?,
        );
    } else {
        info!("No configuration found, creating one using default settings");
        let settings = Settings::default();
        settings
            .save_to_file(&default_config_path)
            .await
            .map_err(|e| {
                format!(
                    "Failed to save default settings to {}: {e}",
                    default_config_path.display()
                )
            })?;
        builder = builder.with_settings(settings);
    }

    // Resolve static directory path for frontend files
    let static_dir = if tauri::is_dev() {
        // Development mode - use source directory
        let dir = "crates/frontend-daemon/dist".to_string();
        info!("Running in Tauri dev mode, using static directory: {}", dir);
        dir
    } else {
        // Production mode - resolve Tauri resources
        let path = app
            .path()
            .resolve("frontend-daemon", BaseDirectory::Resource)
            .map_err(|e| format!("Failed to resolve frontend resources: {e}"))?;
        let dir = path.to_string_lossy().to_string();
        info!("Running in Tauri production mode, resolved static directory: {dir}");
        dir
    };
    builder = builder.with_static_dir(static_dir);

    let daemon = builder
        .build()
        .await
        .map_err(|e| format!("Failed to build daemon: {e}"))?;

    let address = daemon
        .server_address()
        .await
        .map_err(|e| format!("Failed to get server address: {e}"))?;

    // Spawn server task
    let daemon_clone = daemon.clone();
    tokio::spawn(async move {
        if let Err(e) = daemon_clone.serve().await {
            error!("Server error: {}", e);
        }
    });

    // Update the managed state with the new daemon
    app.manage(daemon);

    Ok(format!("Daemon started at http://{address}"))
}

#[tauri::command]
pub async fn stop_daemon(daemon: State<'_, Option<Daemon>>) -> Result<String, String> {
    let daemon = daemon.as_ref().ok_or("Daemon not running")?;
    daemon
        .system_identity()
        .shutdown()
        .await
        .map_err(|e| format!("Failed to shutdown daemon: {e}"))?;

    Ok("Daemon stopped successfully".to_string())
}

#[tauri::command]
pub async fn daemon_status(daemon: State<'_, Option<Daemon>>) -> Result<bool, String> {
    let daemon = daemon.as_ref().ok_or("Daemon not running")?;
    Ok(daemon.status().await.map(|s| s.running).unwrap_or(false))
}

#[tauri::command]
pub async fn get_daemon_config(daemon: State<'_, Daemon>) -> Result<Settings, String> {
    daemon
        .get_settings()
        .await
        .map_err(|e| format!("Failed to get config: {e}"))
}

#[tauri::command]
pub async fn restart_daemon(
    daemon: State<'_, Option<Daemon>>,
    app: AppHandle,
) -> Result<String, String> {
    // Stop if running
    let _ = stop_daemon(daemon).await;
    app.manage(None::<Daemon>);

    // Start with new config
    start_daemon(app).await
}

#[tauri::command]
pub async fn get_daemon_status(daemon: State<'_, Daemon>) -> Result<DaemonStatus, String> {
    daemon
        .status()
        .await
        .map_err(|e| format!("Failed to get status: {e}"))
}

#[tauri::command]
pub async fn get_daemon_runtime_config(
    daemon: State<'_, Daemon>,
) -> Result<DaemonRuntimeConfigResponse, String> {
    let status = daemon
        .status()
        .await
        .map_err(|e| format!("Failed to get status: {e}"))?;

    Ok(DaemonRuntimeConfigResponse {
        server_address: status.listen_address,
        tlsforward_enabled: status.tlsforward_enabled,
    })
}

#[tauri::command]
pub async fn get_tlsforward_status(
    daemon: State<'_, Daemon>,
) -> Result<gate_daemon::TlsForwardStatus, String> {
    let status = daemon
        .status()
        .await
        .map_err(|e| format!("Failed to get status: {e}"))?;
    Ok(status.tlsforward_status)
}

#[tauri::command]
pub async fn configure_tlsforward(
    daemon: State<'_, Daemon>,
    enabled: bool,
    server_address: Option<String>,
) -> Result<String, String> {
    // Get current config
    let mut config = daemon
        .get_settings()
        .await
        .map_err(|e| format!("Failed to get config: {e}"))?;

    // Update TLS forward config
    config.tlsforward.enabled = enabled;
    if let Some(addr) = server_address {
        config.tlsforward.tlsforward_addresses = vec![addr];
    }

    // Update daemon config
    daemon
        .system_identity()
        .update_config(config)
        .await
        .map_err(|e| format!("Failed to update config: {e}"))?;

    Ok("TLS forward configuration updated".to_string())
}

#[tauri::command]
pub async fn enable_tlsforward(daemon: State<'_, Daemon>) -> Result<String, String> {
    configure_tlsforward(daemon, true, None).await
}

#[tauri::command]
pub async fn disable_tlsforward(daemon: State<'_, Daemon>) -> Result<String, String> {
    configure_tlsforward(daemon, false, None).await
}

#[tauri::command]
pub async fn get_bootstrap_url(daemon: State<'_, Daemon>) -> Result<Option<String>, String> {
    daemon
        .bootstrap_url()
        .await
        .map_err(|e| format!("Failed to get bootstrap URL: {e}"))
}

#[tauri::command]
pub async fn get_bootstrap_token(daemon: State<'_, Daemon>) -> Result<Option<String>, String> {
    Ok(daemon
        .get_bootstrap_manager()
        .await
        .map_err(|e| format!("Failed to get bootstrap manager: {e}"))?
        .get_token()
        .await)
}
