use anyhow::Result;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{debug, info};

use crate::{
    Settings, StateDir,
    runtime::{Runtime, inner::RuntimeInner},
};

/// Runtime builder with sensible defaults
#[derive(Default)]
pub struct RuntimeBuilder {
    settings: Option<Settings>,
    state_dir: Option<StateDir>,
    database_url: Option<String>,
    static_dir: Option<String>,
    gui_mode: bool,
}

impl RuntimeBuilder {
    /// Use custom settings
    pub fn with_settings(mut self, settings: Settings) -> Self {
        self.settings = Some(settings);
        self
    }

    /// Use custom state directory
    pub fn with_state_dir(mut self, state_dir: StateDir) -> Self {
        self.state_dir = Some(state_dir);
        self
    }

    /// Use custom database URL
    pub fn with_database_url(mut self, url: String) -> Self {
        self.database_url = Some(url);
        self
    }

    /// Set static files directory for web UI
    pub fn with_static_dir(mut self, dir: String) -> Self {
        self.static_dir = Some(dir);
        self
    }

    /// Enable GUI mode (applies GUI-specific defaults)
    pub fn gui_mode(mut self) -> Self {
        self.gui_mode = true;
        self
    }

    /// Build the runtime
    pub async fn build(self) -> Result<Runtime> {
        info!(
            "Building runtime in {} mode",
            if self.gui_mode { "GUI" } else { "daemon" }
        );

        // Get or create state directory
        let state_dir = self.state_dir.unwrap_or_default();
        state_dir.create_directories().await?;

        // Get or create settings
        let mut settings = if let Some(settings) = self.settings {
            settings
        } else if self.gui_mode {
            debug!("Using GUI preset configuration");
            Settings::gui_preset()
        } else {
            // Try to load from default location
            let config_path = state_dir.config_path();
            if config_path.exists() {
                info!("Loading configuration from: {}", config_path.display());
                Settings::load_from_file(&config_path.to_string_lossy())?
            } else {
                debug!("Using daemon preset configuration");
                Settings::daemon_preset()
            }
        };

        // Apply GUI overrides if in GUI mode
        if self.gui_mode {
            settings.apply_gui_overrides();
        }

        // Get database URL
        let database_url = self.database_url.unwrap_or_else(|| {
            format!(
                "sqlite://{}",
                state_dir.data_dir().join("gate.db").display()
            )
        });

        // Get static directory
        let static_dir = self.static_dir.or_else(|| {
            if self.gui_mode {
                Some("crates/frontend-daemon/dist".to_string())
            } else {
                std::env::var("GATE_SERVER__STATIC_DIR")
                    .ok()
                    .or_else(|| Some("crates/frontend-daemon/dist".to_string()))
            }
        });

        // Create shutdown channel
        let (shutdown_tx, _) = watch::channel(false);

        // Initialize runtime inner
        let inner = RuntimeInner::initialize(settings, state_dir, database_url, static_dir).await?;

        Ok(Runtime {
            inner: Arc::new(inner),
            shutdown_tx,
        })
    }
}
