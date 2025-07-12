//! Platform-specific state directory management

use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::PathBuf;
use tracing::{debug, warn};

/// Manages platform-specific application directories
pub struct StateDir {
    /// Project directories from the directories crate
    project_dirs: Option<ProjectDirs>,
    /// Override directory for testing or custom installations
    override_dir: Option<PathBuf>,
}

impl StateDir {
    /// Create a new StateDir instance
    pub fn new() -> Self {
        let project_dirs = ProjectDirs::from("com.hellas", "Gate", "Gate");
        if project_dirs.is_none() {
            warn!("Failed to determine platform-specific directories, will use fallback");
        }
        Self {
            project_dirs,
            override_dir: None,
        }
    }

    /// Create a new StateDir with an override directory
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

    /// Get the cache directory
    pub fn cache_dir(&self) -> PathBuf {
        if let Some(override_dir) = &self.override_dir {
            return override_dir.join("cache");
        }

        if let Some(project_dirs) = &self.project_dirs {
            project_dirs.cache_dir().to_path_buf()
        } else {
            // Fallback to current directory
            PathBuf::from("./cache")
        }
    }

    /// Get the directory for storing keys
    pub fn keys_dir(&self) -> PathBuf {
        self.data_dir().join("keys")
    }

    /// Get the directory for storing certificates
    pub fn certificates_dir(&self) -> PathBuf {
        self.data_dir().join("certificates")
    }

    /// Get the directory for ACME account data
    pub fn acme_dir(&self) -> PathBuf {
        self.data_dir().join("acme")
    }

    /// Get the path for the Iroh secret key
    pub fn iroh_secret_key_path(&self) -> PathBuf {
        self.keys_dir().join("iroh_secret.key")
    }

    /// Get the config path
    pub fn config_path(&self) -> PathBuf {
        self.config_dir().join("config.json")
    }

    /// Create all required directories
    pub async fn create_directories(&self) -> Result<()> {
        let dirs = vec![
            self.config_dir(),
            self.data_dir(),
            self.cache_dir(),
            self.keys_dir(),
            self.certificates_dir(),
            self.acme_dir(),
        ];

        for dir in dirs {
            tokio::fs::create_dir_all(&dir)
                .await
                .with_context(|| format!("Failed to create directory: {}", dir.display()))?;
            debug!("Ensured directory exists: {}", dir.display());
        }

        debug!("Using state directories:");
        debug!("  Config: {}", self.config_dir().display());
        debug!("  Data: {}", self.data_dir().display());
        debug!("  Cache: {}", self.cache_dir().display());

        Ok(())
    }
}

impl Default for StateDir {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_override_directory() {
        let temp_dir = TempDir::new().unwrap();
        let state_dir = StateDir::with_override(temp_dir.path());

        assert_eq!(state_dir.config_dir(), temp_dir.path().join("config"));
        assert_eq!(state_dir.data_dir(), temp_dir.path().join("data"));
        assert_eq!(state_dir.cache_dir(), temp_dir.path().join("cache"));
    }

    #[tokio::test]
    async fn test_create_directories() {
        let temp_dir = TempDir::new().unwrap();
        let state_dir = StateDir::with_override(temp_dir.path());

        state_dir.create_directories().await.unwrap();

        assert!(state_dir.config_dir().exists());
        assert!(state_dir.data_dir().exists());
        assert!(state_dir.cache_dir().exists());
        assert!(state_dir.keys_dir().exists());
        assert!(state_dir.certificates_dir().exists());
        assert!(state_dir.acme_dir().exists());
    }
}
