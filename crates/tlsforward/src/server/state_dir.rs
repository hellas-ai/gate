//! Platform-specific state directory management for TLS forward server

use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::PathBuf;

/// Manages platform-specific application directories for the TLS forward server
pub struct TlsForwardStateDir {
    /// Project directories from the directories crate
    project_dirs: Option<ProjectDirs>,
    /// Override directory for testing or custom installations
    override_dir: Option<PathBuf>,
}

impl TlsForwardStateDir {
    /// Create a new TlsForwardStateDir instance
    pub fn new() -> Self {
        let project_dirs = ProjectDirs::from("com.hellas", "Gate", "Gate-TlsForward");

        if project_dirs.is_none() {
            warn!("Failed to determine platform-specific directories, will use fallback");
        }

        Self {
            project_dirs,
            override_dir: None,
        }
    }

    /// Create a new TlsForwardStateDir with an override directory
    pub fn with_override(path: impl Into<PathBuf>) -> Self {
        Self {
            project_dirs: None,
            override_dir: Some(path.into()),
        }
    }

    /// Get the configuration directory
    pub fn config_dir(&self) -> PathBuf {
        if let Some(override_dir) = &self.override_dir {
            return override_dir.join("config");
        }

        if let Some(project_dirs) = &self.project_dirs {
            project_dirs.config_dir().to_path_buf()
        } else {
            // Fallback to current directory
            PathBuf::from("./config")
        }
    }

    /// Get the data directory for persistent storage
    pub fn data_dir(&self) -> PathBuf {
        if let Some(override_dir) = &self.override_dir {
            return override_dir.join("data");
        }

        if let Some(project_dirs) = &self.project_dirs {
            project_dirs.data_dir().to_path_buf()
        } else {
            // Fallback to current directory
            PathBuf::from("./data")
        }
    }

    /// Get the directory for storing keys
    pub fn keys_dir(&self) -> PathBuf {
        self.data_dir().join("keys")
    }

    /// Get the path for the relay secret key
    pub fn secret_key_path(&self) -> PathBuf {
        self.keys_dir().join("secret.key")
    }

    /// Create all required directories
    pub async fn create_directories(&self) -> Result<()> {
        let dirs = vec![self.config_dir(), self.data_dir(), self.keys_dir()];

        for dir in dirs {
            tokio::fs::create_dir_all(&dir)
                .await
                .with_context(|| format!("Failed to create directory: {}", dir.display()))?;
            debug!("Ensured directory exists: {}", dir.display());
        }

        info!("Created relay state directories:");
        info!("  Config: {}", self.config_dir().display());
        info!("  Data: {}", self.data_dir().display());

        Ok(())
    }
}

impl Default for TlsForwardStateDir {
    fn default() -> Self {
        Self::new()
    }
}
