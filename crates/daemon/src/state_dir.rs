//! Platform-specific state directory management
use crate::DaemonError;
use directories::ProjectDirs;
use std::path::PathBuf;

/// Manages platform-specific application directories
pub struct StateDir(ProjectDirs);

impl StateDir {
    /// Create a new StateDir instance
    pub async fn new() -> Result<Self, DaemonError> {
        let dirs = ProjectDirs::from("com.hellas", "Gate", "Gate")
            .ok_or(DaemonError::PlatformDirsNotFound)?;
        let me = Self(dirs);
        me.create_directories().await?;
        Ok(me)
    }

    /// Create all required directories
    pub async fn create_directories(&self) -> Result<(), DaemonError> {
        let config_dir = self.0.config_local_dir();
        let data_dir = self.0.data_local_dir();

        let dirs_to_create = vec![config_dir, data_dir];
        for dir in &dirs_to_create {
            tokio::fs::create_dir_all(&dir).await?;
            debug!("Ensured directory exists: {}", dir.display());
        }

        debug!("Using state directories:");
        debug!("  Config: {}", config_dir.display());
        debug!("  Data: {}", data_dir.display());

        Ok(())
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
}
