use gate_daemon::{Settings, runtime::Runtime};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::debug;

/// GUI-specific daemon state
pub struct DaemonState {
    pub(crate) runtime: Arc<RwLock<Option<Runtime>>>,
    pub(crate) server_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    pub(crate) config_path: PathBuf,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonState {
    pub fn new() -> Self {
        let state_dir = gate_daemon::StateDir::new();
        let config_path = state_dir.config_dir().join("gui-config.json");

        Self {
            runtime: Arc::new(RwLock::new(None)),
            server_handle: Arc::new(RwLock::new(None)),
            config_path,
        }
    }

    pub async fn is_running(&self) -> bool {
        self.server_handle.read().await.is_some()
    }

    pub async fn set_runtime(&self, runtime: Runtime) {
        *self.runtime.write().await = Some(runtime);
    }

    pub async fn set_handle(&self, handle: JoinHandle<()>) {
        *self.server_handle.write().await = Some(handle);
    }

    pub async fn get_runtime(&self) -> Option<Runtime> {
        self.runtime.read().await.clone()
    }

    pub async fn shutdown(&self) {
        // Shutdown runtime
        if let Some(runtime) = self.runtime.write().await.take() {
            runtime.shutdown().await;
        }

        // Wait for server handle
        if let Some(handle) = self.server_handle.write().await.take() {
            let _ = tokio::time::timeout(tokio::time::Duration::from_secs(5), handle).await;
        }
    }

    pub fn load_config(&self) -> Result<Settings, anyhow::Error> {
        if self.config_path.exists() {
            let contents = std::fs::read_to_string(&self.config_path)?;
            let config: Settings = serde_json::from_str(&contents)?;
            debug!("Loaded GUI config from {}", self.config_path.display());
            Ok(config)
        } else {
            debug!(
                "No existing GUI config found at {}",
                self.config_path.display()
            );
            Ok(Settings::gui_preset())
        }
    }

    pub async fn save_config(&self, config: &Settings) -> Result<(), anyhow::Error> {
        // Ensure directory exists
        if let Some(parent) = self.config_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let contents = serde_json::to_string_pretty(config)?;
        tokio::fs::write(&self.config_path, contents).await?;
        debug!("Saved GUI config to {}", self.config_path.display());
        Ok(())
    }
}

/// Simplified TLS forward status for UI
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum TlsForwardStatus {
    Disabled,
    Disconnected,
    Connecting,
    Connected { domain: String },
    Error(String),
}
