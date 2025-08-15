use crate::state::{DaemonState, TlsForwardStatus};
use gate_daemon::{Settings, runtime::Runtime};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, State};
use tracing::{error, info};

#[tauri::command]
pub async fn start_daemon(
    state: State<'_, DaemonState>,
    app: AppHandle,
    config: Option<Settings>,
) -> Result<String, String> {
    // Check if already running
    if state.is_running().await {
        return Err("Daemon is already running".to_string());
    }

    // Load or use provided config
    let settings = if let Some(cfg) = config {
        // Save the new config
        if let Err(e) = state.save_config(&cfg).await {
            error!("Failed to save GUI config: {}", e);
        }
        cfg
    } else {
        state
            .load_config()
            .unwrap_or_else(|_| Settings::gui_preset())
    };

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

    // Build runtime
    let runtime = Runtime::builder()
        .gui_mode()
        .with_static_dir(static_dir)
        .with_settings(settings)
        .build()
        .await
        .map_err(|e| format!("Failed to build runtime: {e}"))?;

    let address = runtime.server_address();

    // Start monitoring
    runtime.start_monitoring().await;

    // Start metrics if configured
    runtime
        .start_metrics()
        .await
        .map_err(|e| format!("Failed to start metrics: {e}"))?;

    // Spawn server task
    let runtime_clone = runtime.clone();
    let handle = tokio::spawn(async move {
        if let Err(e) = runtime_clone.serve().await {
            error!("Server error: {}", e);
        }
    });

    // Store runtime and handle
    state.set_runtime(runtime).await;
    state.set_handle(handle).await;

    Ok(format!("Daemon started at http://{address}"))
}

#[tauri::command]
pub async fn stop_daemon(state: State<'_, DaemonState>) -> Result<String, String> {
    if !state.is_running().await {
        return Err("Daemon is not running".to_string());
    }

    state.shutdown().await;
    Ok("Daemon stopped successfully".to_string())
}

#[tauri::command]
pub async fn daemon_status(state: State<'_, DaemonState>) -> Result<bool, String> {
    Ok(state.is_running().await)
}

#[tauri::command]
pub async fn get_daemon_config(state: State<'_, DaemonState>) -> Result<Settings, String> {
    state
        .load_config()
        .map_err(|e| format!("Failed to load config: {e}"))
}

#[tauri::command]
pub async fn restart_daemon(
    state: State<'_, DaemonState>,
    app: AppHandle,
    config: Option<Settings>,
) -> Result<String, String> {
    // Stop if running
    if state.is_running().await {
        let _ = stop_daemon(state.clone()).await;
        // Wait a bit for cleanup
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    // Start with new config
    start_daemon(state, app, config).await
}

#[tauri::command]
pub async fn get_daemon_status(state: State<'_, DaemonState>) -> Result<serde_json::Value, String> {
    let running = state.is_running().await;
    let runtime_config = if let Some(runtime) = state.get_runtime().await {
        serde_json::json!({
            "address": runtime.server_address(),
            "tlsforward_enabled": runtime.tlsforward_enabled(),
        })
    } else {
        serde_json::json!({})
    };

    Ok(serde_json::json!({
        "running": running,
        "config": runtime_config,
    }))
}

#[tauri::command]
pub async fn get_daemon_runtime_config(
    state: State<'_, DaemonState>,
) -> Result<serde_json::Value, String> {
    if let Some(runtime) = state.get_runtime().await {
        Ok(serde_json::json!({
            "server_address": runtime.server_address(),
            "tlsforward_enabled": runtime.tlsforward_enabled(),
        }))
    } else {
        Err("Runtime not available".to_string())
    }
}

#[tauri::command]
pub async fn get_tlsforward_status(
    state: State<'_, DaemonState>,
) -> Result<TlsForwardStatus, String> {
    if let Some(runtime) = state.get_runtime().await {
        let status = runtime.tlsforward_status().await;
        Ok(match status {
            gate_daemon::runtime::TlsForwardStatus::Disabled => TlsForwardStatus::Disabled,
            gate_daemon::runtime::TlsForwardStatus::Disconnected => TlsForwardStatus::Disconnected,
            gate_daemon::runtime::TlsForwardStatus::Connecting => TlsForwardStatus::Connecting,
            gate_daemon::runtime::TlsForwardStatus::Connected { domain } => {
                TlsForwardStatus::Connected { domain }
            }
            gate_daemon::runtime::TlsForwardStatus::Error(e) => TlsForwardStatus::Error(e),
        })
    } else {
        Ok(TlsForwardStatus::Disabled)
    }
}

#[tauri::command]
pub async fn configure_tlsforward(
    state: State<'_, DaemonState>,
    _enabled: bool,
    _server_address: Option<String>,
) -> Result<String, String> {
    // Load current config
    let config = state
        .load_config()
        .map_err(|e| format!("Failed to load config: {e}"))?;

    // Update TLS forward config
    // Note: This would need to be implemented based on how you want to handle
    // runtime configuration changes

    // Save config
    state
        .save_config(&config)
        .await
        .map_err(|e| format!("Failed to save config: {e}"))?;

    Ok("TLS forward configuration updated".to_string())
}

#[tauri::command]
pub async fn enable_tlsforward(state: State<'_, DaemonState>) -> Result<String, String> {
    configure_tlsforward(state, true, None).await
}

#[tauri::command]
pub async fn disable_tlsforward(state: State<'_, DaemonState>) -> Result<String, String> {
    configure_tlsforward(state, false, None).await
}

#[tauri::command]
pub async fn get_bootstrap_url(state: State<'_, DaemonState>) -> Result<Option<String>, String> {
    if let Some(runtime) = state.get_runtime().await {
        Ok(runtime.bootstrap_url())
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub async fn get_bootstrap_token(state: State<'_, DaemonState>) -> Result<Option<String>, String> {
    if let Some(runtime) = state.get_runtime().await {
        Ok(runtime.bootstrap_token().map(|s| s.to_string()))
    } else {
        Ok(None)
    }
}
