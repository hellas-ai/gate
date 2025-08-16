#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

// Tauri v2 API
#[wasm_bindgen(inline_js = "
export function invoke_tauri_command(cmd, args) {
    if (window.__TAURI__?.core?.invoke) {
        return window.__TAURI__.core.invoke(cmd, args);
    }
    console.error('Tauri API not found. Ensure withGlobalTauri is set to true in tauri.conf.json');
    return Promise.reject('Tauri API not available');
}

export function check_tauri_available() {
    return !!(window.__TAURI__?.core?.invoke);
}
")]
extern "C" {
    #[wasm_bindgen(js_name = invoke_tauri_command)]
    async fn tauri_invoke(cmd: &str, args: JsValue) -> JsValue;

    #[wasm_bindgen(js_name = check_tauri_available)]
    fn check_tauri() -> bool;
}

// Helper to log invoke calls
async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, String> {
    web_sys::console::log_1(&format!("Invoking Tauri command: {cmd}").into());

    // Check if Tauri is available
    if !check_tauri() {
        web_sys::console::error_1(&"Tauri API not available!".into());
        return Err("Tauri API not available".to_string());
    }

    let result = tauri_invoke(cmd, args).await;
    web_sys::console::log_1(&format!("Tauri command {cmd} completed").into());
    Ok(result)
}

// Minimal Settings struct that matches backend
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub server: ServerConfig,
    pub letsencrypt: LetsEncryptConfig,
    pub tlsforward: TlsForwardConfig,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct LetsEncryptConfig {
    pub enabled: bool,
    pub email: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct TlsForwardConfig {
    pub enabled: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct DaemonRuntimeStatus {
    pub running: bool,
    pub listen_address: Option<String>,
    pub has_upstreams: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub enum TlsForwardState {
    Disabled,
    Disconnected,
    Connecting,
    Connected {
        server_address: String,
        assigned_domain: String,
    },
    Error(String),
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct DaemonRuntimeConfig {
    pub listen_address: String,
    pub database_url: String,
    pub upstream_count: usize,
    pub auth_enabled: bool,
    pub webauthn_enabled: bool,
    pub p2p_node_id: Option<String>,
    pub p2p_listen_addresses: Vec<String>,
    pub tlsforward_enabled: bool,
    pub tlsforward_state: Option<TlsForwardState>,
    pub needs_bootstrap: bool,
}

/// Start the daemon
pub async fn start_daemon() -> Result<String, String> {
    let result = invoke("start_daemon", JsValue::UNDEFINED).await?;
    serde_wasm_bindgen::from_value::<String>(result).map_err(|e| e.to_string())
}

/// Stop the daemon
pub async fn stop_daemon() -> Result<String, String> {
    let result = invoke("stop_daemon", JsValue::UNDEFINED).await?;
    serde_wasm_bindgen::from_value::<String>(result).map_err(|e| e.to_string())
}

/// Check if daemon is running
pub async fn daemon_status() -> Result<bool, String> {
    let result = invoke("daemon_status", JsValue::UNDEFINED).await?;
    serde_wasm_bindgen::from_value::<bool>(result).map_err(|e| e.to_string())
}

/// Get daemon configuration
pub async fn get_daemon_config() -> Result<Settings, String> {
    let result = invoke("get_daemon_config", JsValue::UNDEFINED).await?;
    serde_wasm_bindgen::from_value::<Settings>(result).map_err(|e| e.to_string())
}

/// Get daemon runtime status
pub async fn get_daemon_status() -> Result<DaemonRuntimeStatus, String> {
    web_sys::console::log_1(&"Getting daemon status...".into());
    let result = invoke("get_daemon_status", JsValue::UNDEFINED).await?;
    serde_wasm_bindgen::from_value::<DaemonRuntimeStatus>(result).map_err(|e| {
        web_sys::console::error_1(&format!("Failed to deserialize status: {e}").into());
        e.to_string()
    })
}

/// Get daemon runtime configuration
pub async fn get_daemon_runtime_config() -> Result<DaemonRuntimeConfig, String> {
    let result = invoke("get_daemon_runtime_config", JsValue::UNDEFINED).await?;
    serde_wasm_bindgen::from_value::<DaemonRuntimeConfig>(result).map_err(|e| e.to_string())
}

/// Restart daemon
pub async fn restart_daemon() -> Result<String, String> {
    let result = invoke("restart_daemon", JsValue::UNDEFINED).await?;

    serde_wasm_bindgen::from_value::<String>(result).map_err(|e| e.to_string())
}

/// Get TLS forward status
pub async fn get_tlsforward_status() -> Result<Option<TlsForwardState>, String> {
    let result = invoke("get_tlsforward_status", JsValue::UNDEFINED).await?;

    serde_wasm_bindgen::from_value::<Option<TlsForwardState>>(result).map_err(|e| e.to_string())
}

/// Configure TLS forward with email
pub async fn configure_tlsforward(email: String) -> Result<String, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "email": email }))
        .map_err(|e| e.to_string())?;

    let result = invoke("configure_tlsforward", args).await?;

    serde_wasm_bindgen::from_value::<String>(result).map_err(|e| e.to_string())
}

/// Enable TLS forward
pub async fn enable_tlsforward() -> Result<String, String> {
    let result = invoke("enable_tlsforward", JsValue::UNDEFINED).await?;

    serde_wasm_bindgen::from_value::<String>(result).map_err(|e| e.to_string())
}

/// Disable TLS forward
pub async fn disable_tlsforward() -> Result<String, String> {
    let result = invoke("disable_tlsforward", JsValue::UNDEFINED).await?;

    serde_wasm_bindgen::from_value::<String>(result).map_err(|e| e.to_string())
}

/// Open URL in default browser using Tauri opener plugin
pub async fn open_url(url: String) -> Result<(), String> {
    web_sys::console::log_1(&format!("Attempting to open URL: {url}").into());

    // Validate URL to restrict to Hellas private URLs only
    if !is_allowed_url(&url) {
        web_sys::console::error_1(&format!("URL not allowed: {url}").into());
        return Err(format!("URL not allowed: {url}"));
    }

    // Use the opener plugin through Tauri's invoke system
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "url": url }))
        .map_err(|e| e.to_string())?;

    match invoke("plugin:opener|open_url", args).await {
        Ok(_) => {
            web_sys::console::log_1(&format!("Successfully opened URL: {url}").into());
            Ok(())
        }
        Err(e) => {
            web_sys::console::error_1(&format!("Failed to open URL: {e}").into());
            Err(format!("Failed to open URL: {e}"))
        }
    }
}

/// Get bootstrap token for initial admin setup
pub async fn get_bootstrap_token() -> Result<Option<String>, String> {
    let result = invoke("get_bootstrap_token", JsValue::UNDEFINED).await?;
    serde_wasm_bindgen::from_value::<Option<String>>(result).map_err(|e| e.to_string())
}

/// Check if a URL is allowed to be opened
fn is_allowed_url(url: &str) -> bool {
    // Allow localhost URLs for development
    if url.starts_with("http://localhost") || url.starts_with("https://localhost") {
        return true;
    }

    // Allow Hellas AI domains
    if url.starts_with("https://hellas.ai") || url.starts_with("https://*.hellas.ai") {
        return true;
    }

    // Allow Hellas private/internal URLs
    if url.contains(".hellas.") || url.contains("hellas-") {
        return true;
    }

    // Allow Gate-specific domains
    if url.contains("gate.") || url.contains("-gate.") {
        return true;
    }

    // Deny everything else
    false
}
