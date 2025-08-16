//! Platform-specific state directory management
use crate::DaemonError;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::PathBuf;

/// Manages platform-specific application directories
pub struct StateDir(ProjectDirs);

impl StateDir {
    /// Create a new StateDir instance
    pub fn new() -> Result<Self, DaemonError> {
        let dirs = ProjectDirs::from("com.hellas", "Gate", "Gate")
            .ok_or(DaemonError::PlatformDirsNotFound)?;
        Ok(Self(dirs))
    }

    /// Get the configuration directory
    pub fn config_dir(&self) -> PathBuf {
        self.0.config_local_dir().to_path_buf()
    }

    /// Get the data directory for persistent storage
    pub fn data_dir(&self) -> PathBuf {
        self.0.data_local_dir().to_path_buf()
    }

    /// Get the directory for storing keys
    pub fn dir_for(&self, component: &str) -> PathBuf {
        self.data_dir().join(component)
    }

    /// Get the config path
    pub fn config_path(&self) -> PathBuf {
        self.config_dir().join("config.json")
    }

    /// Get the path for the Iroh secret key
    pub fn iroh_secret_key_path(&self) -> PathBuf {
        self.config_dir().join("iroh_secret.key")
    }

    /// Create all required directories
    pub async fn create_directories(&self) -> Result<()> {
        let dirs = vec![self.config_dir(), self.data_dir()];
        for dir in dirs {
            tokio::fs::create_dir_all(&dir)
                .await
                .with_context(|| format!("Failed to create directory: {}", dir.display()))?;
            debug!("Ensured directory exists: {}", dir.display());
        }

        debug!("Using state directories:");
        debug!("  Config: {}", self.config_dir().display());
        debug!("  Data: {}", self.data_dir().display());

        Ok(())
    }
}
