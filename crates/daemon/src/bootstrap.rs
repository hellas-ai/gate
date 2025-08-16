use anyhow::Result;
use gate_sqlx::SqliteWebAuthnBackend;
use rand::{Rng, distributions::Alphanumeric};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Manages the bootstrap token for initial user enrollment
#[derive(Clone)]
pub struct BootstrapTokenManager {
    inner: Arc<RwLock<BootstrapTokenState>>,
    webauthn_backend: Arc<SqliteWebAuthnBackend>,
}

#[derive(Debug)]
struct BootstrapTokenState {
    token: Option<String>,
    is_used: bool,
}

impl BootstrapTokenManager {
    /// Creates a new bootstrap token manager
    pub fn new(webauthn_backend: Arc<SqliteWebAuthnBackend>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(BootstrapTokenState {
                token: None,
                is_used: false,
            })),
            webauthn_backend,
        }
    }

    /// Generates a new bootstrap token if none exists and hasn't been used
    pub async fn generate_token(&self) -> Result<String> {
        let mut state = self.inner.write().await;

        if state.is_used {
            anyhow::bail!("Bootstrap token has already been used");
        }

        if let Some(ref token) = state.token {
            return Ok(token.clone());
        }

        // Generate a 32-character random alphanumeric token
        let token: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();

        state.token = Some(token.clone());
        Ok(token)
    }

    /// Validates a bootstrap token
    pub async fn validate_token(&self, token: &str) -> Result<bool> {
        let state = self.inner.read().await;

        if state.is_used {
            return Ok(false);
        }

        match &state.token {
            Some(stored_token) => Ok(stored_token == token),
            None => Ok(false),
        }
    }

    /// Marks the bootstrap token as used
    pub async fn mark_as_used(&self) -> Result<()> {
        let mut state = self.inner.write().await;

        if state.token.is_none() {
            anyhow::bail!("No bootstrap token has been generated");
        }

        state.is_used = true;
        state.token = None; // Clear the token for security
        Ok(())
    }

    /// Checks if the bootstrap process has been completed
    pub async fn is_bootstrap_complete(&self) -> bool {
        let state = self.inner.read().await;
        state.is_used
    }

    /// Gets the current token if available and not used
    pub async fn get_token(&self) -> Option<String> {
        let state = self.inner.read().await;
        if !state.is_used {
            state.token.clone()
        } else {
            None
        }
    }

    /// Checks if bootstrap is needed (no credentials exist)
    pub async fn needs_bootstrap(&self) -> Result<bool> {
        let credentials = self.webauthn_backend.list_all_credentials().await?;
        Ok(credentials.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gate_sqlx::{SqliteStateBackend, SqliteWebAuthnBackend};

    async fn create_test_manager() -> Arc<BootstrapTokenManager> {
        let state_backend = Arc::new(
            SqliteStateBackend::new(":memory:")
                .await
                .expect("Failed to create backend"),
        );
        let webauthn_backend = Arc::new(SqliteWebAuthnBackend::new(state_backend.pool().clone()));
        Arc::new(BootstrapTokenManager::new(webauthn_backend))
    }

    #[tokio::test]
    async fn test_bootstrap_token_generation() {
        let manager = create_test_manager().await;

        // First token generation should succeed
        let token1 = manager
            .generate_token()
            .await
            .expect("Failed to generate token");
        assert!(!token1.is_empty());

        // Second generation should return the same token
        let token2 = manager
            .generate_token()
            .await
            .expect("Failed to generate token");
        assert_eq!(token1, token2);
    }

    #[tokio::test]
    async fn test_bootstrap_token_validation() {
        let manager = create_test_manager().await;

        // Generate a token
        let token = manager
            .generate_token()
            .await
            .expect("Failed to generate token");

        // Valid token should pass
        let valid = manager
            .validate_token(&token)
            .await
            .expect("Failed to validate token");
        assert!(valid);

        // Invalid token should fail
        let invalid = manager
            .validate_token("invalid-token")
            .await
            .expect("Failed to validate token");
        assert!(!invalid);
    }

    #[tokio::test]
    async fn test_bootstrap_token_single_use() {
        let manager = create_test_manager().await;

        // Generate and validate token
        let token = manager
            .generate_token()
            .await
            .expect("Failed to generate token");

        let valid = manager
            .validate_token(&token)
            .await
            .expect("Failed to validate token");
        assert!(valid);

        // Mark as used
        manager
            .mark_as_used()
            .await
            .expect("Failed to mark token as used");

        // Token should no longer be valid
        let valid_after = manager
            .validate_token(&token)
            .await
            .expect("Failed to validate token");
        assert!(!valid_after);

        // Should not be able to generate new token after use
        let result = manager.generate_token().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_bootstrap_status() {
        let manager = create_test_manager().await;

        // Initially should need bootstrap
        let needs = manager
            .needs_bootstrap()
            .await
            .expect("Failed to check bootstrap status");
        assert!(needs);

        // Should not be complete
        let complete = manager.is_bootstrap_complete().await;
        assert!(!complete);

        // Generate token
        let _token = manager
            .generate_token()
            .await
            .expect("Failed to generate token");

        // Still needs bootstrap (token generated but not used)
        let needs = manager
            .needs_bootstrap()
            .await
            .expect("Failed to check bootstrap status");
        assert!(needs);

        // Mark as used
        manager
            .mark_as_used()
            .await
            .expect("Failed to mark token as used");

        // Should no longer need bootstrap
        let needs = manager
            .needs_bootstrap()
            .await
            .expect("Failed to check bootstrap status");
        // This will still be true because we haven't created any credentials
        // In real usage, a user would be created after token validation
        assert!(needs);

        // Should be complete
        let complete = manager.is_bootstrap_complete().await;
        assert!(complete);
    }
}
