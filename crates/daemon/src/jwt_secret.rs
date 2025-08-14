//! JWT secret key management
//!
//! This module handles secure generation and persistence of JWT signing secrets.

use anyhow::{Context, Result};
use rand::{Rng, distributions::Alphanumeric};
use std::path::Path;
use tokio::fs;
use tracing::{debug, info};

/// Generate a new cryptographically secure JWT secret
fn generate_jwt_secret() -> String {
    // Generate a 64-byte (512-bit) random secret
    // Using alphanumeric characters for compatibility
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect()
}

/// Load or create a JWT secret key
///
/// If the file exists, loads the secret from it.
/// If not, generates a new secret and saves it.
pub async fn load_or_create_jwt_secret(path: &Path) -> Result<String> {
    if path.exists() {
        debug!("Loading existing JWT secret from: {}", path.display());
        let secret = fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read JWT secret from {}", path.display()))?;

        // Validate the secret has reasonable length
        if secret.trim().len() < 32 {
            anyhow::bail!("JWT secret is too short (minimum 32 characters)");
        }

        Ok(secret.trim().to_string())
    } else {
        info!("Generating new JWT secret at: {}", path.display());

        let secret = generate_jwt_secret();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .with_context(|| "Failed to create directory for JWT secret")?;
        }

        // Write secret with restricted permissions
        fs::write(path, &secret)
            .await
            .with_context(|| format!("Failed to write JWT secret to {}", path.display()))?;

        // On Unix, set restrictive permissions (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(0o600);
            fs::set_permissions(path, permissions)
                .await
                .with_context(|| "Failed to set permissions on JWT secret file")?;
        }

        // On Windows, set file as read-only for current user
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            let mut permissions = fs::metadata(path).await?.permissions();
            permissions.set_readonly(true);
            fs::set_permissions(path, permissions)
                .await
                .with_context(|| "Failed to set permissions on JWT secret file")?;
        }

        info!("JWT secret generated and saved successfully");
        Ok(secret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_generate_jwt_secret() {
        let secret1 = generate_jwt_secret();
        let secret2 = generate_jwt_secret();

        // Should be 64 characters long
        assert_eq!(secret1.len(), 64);
        assert_eq!(secret2.len(), 64);

        // Should be different each time
        assert_ne!(secret1, secret2);

        // Should only contain alphanumeric characters
        assert!(secret1.chars().all(|c| c.is_alphanumeric()));
    }

    #[tokio::test]
    async fn test_load_or_create_jwt_secret() {
        let temp_dir = TempDir::new().unwrap();
        let secret_path = temp_dir.path().join("jwt_secret.key");

        // First call should create
        let secret1 = load_or_create_jwt_secret(&secret_path).await.unwrap();
        assert_eq!(secret1.len(), 64);
        assert!(secret_path.exists());

        // Second call should load the same secret
        let secret2 = load_or_create_jwt_secret(&secret_path).await.unwrap();
        assert_eq!(secret1, secret2);
    }

    #[tokio::test]
    async fn test_reject_short_secret() {
        let temp_dir = TempDir::new().unwrap();
        let secret_path = temp_dir.path().join("jwt_secret.key");

        // Write a short secret
        fs::write(&secret_path, "too-short").await.unwrap();

        // Should fail to load
        let result = load_or_create_jwt_secret(&secret_path).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too short"));
    }
}
