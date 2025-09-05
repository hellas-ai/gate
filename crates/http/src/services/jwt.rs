//! JWT service for token management

use crate::error::HttpError;
use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// JWT Claims structure
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: String,
    /// User's display name
    pub name: Option<String>,
    /// Expiration time (as UTC timestamp)
    pub exp: i64,
    /// Issued at (as UTC timestamp)
    pub iat: i64,
    /// Issuer
    pub iss: String,
}

/// JWT service configuration
#[derive(Clone, Debug)]
pub struct JwtConfig {
    /// Secret key for signing tokens
    pub secret: String,
    /// Token expiration duration
    pub expiration: Duration,
    /// Token issuer
    pub issuer: String,
}

impl JwtConfig {
    /// Default configuration with 24 hours expiration
    pub fn new(secret: String, expiration_hours: i64, issuer: String) -> Self {
        Self {
            secret,
            expiration: Duration::hours(expiration_hours),
            issuer,
        }
    }
}

/// JWT service for token operations
pub struct JwtService {
    config: Arc<JwtConfig>,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
}

impl JwtService {
    /// Create a new JWT service
    pub fn new(config: JwtConfig) -> Self {
        let encoding_key = EncodingKey::from_secret(config.secret.as_bytes());
        let decoding_key = DecodingKey::from_secret(config.secret.as_bytes());

        Self {
            config: Arc::new(config),
            encoding_key,
            decoding_key,
        }
    }

    /// Generate a JWT token for a user
    pub fn generate_token(&self, user_id: &str, name: Option<&str>) -> Result<String, HttpError> {
        let now = Utc::now();
        let expiration = now + self.config.expiration;

        let claims = Claims {
            sub: user_id.to_string(),
            name: name.map(|s| s.to_string()),
            exp: expiration.timestamp(),
            iat: now.timestamp(),
            iss: self.config.issuer.clone(),
        };

        let header = Header::new(Algorithm::HS256);

        encode(&header, &claims, &self.encoding_key)
            .map_err(|e| HttpError::InternalServerError(format!("Failed to generate token: {e}")))
    }

    /// Validate a JWT token and extract claims
    pub fn validate_token(&self, token: &str) -> Result<Claims, HttpError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(std::slice::from_ref(&self.config.issuer));
        decode::<Claims>(token, &self.decoding_key, &validation)
            .map(|token_data| token_data.claims)
            .map_err(|e| match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => {
                    HttpError::AuthenticationFailed("Token has expired".to_string())
                }
                jsonwebtoken::errors::ErrorKind::InvalidToken => {
                    HttpError::AuthenticationFailed("Invalid token".to_string())
                }
                _ => HttpError::AuthenticationFailed(format!("Token validation failed: {e}")),
            })
    }

    /// Extract token from Authorization header
    pub fn extract_bearer_token<'a>(&self, auth_header: &'a str) -> Result<&'a str, HttpError> {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            Ok(token)
        } else {
            Err(HttpError::AuthenticationFailed(
                "Invalid authorization header format".to_string(),
            ))
        }
    }

    /// Get the token expiration duration
    pub fn expiration(&self) -> Duration {
        self.config.expiration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_generation_and_validation() {
        let config = JwtConfig::new("test-secret".to_string(), 24, "test-issuer".to_string());
        let service = JwtService::new(config);
        let user_id = "test-user-123";
        let name = Some("Test User");

        // Generate token
        let token = service.generate_token(user_id, name).unwrap();
        assert!(!token.is_empty());

        // Validate token
        let claims = service.validate_token(&token).unwrap();
        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.name.as_deref(), name);
    }

    #[test]
    fn test_expired_token() {
        let config = JwtConfig::new("test-secret".to_string(), 24, "test-issuer".to_string());
        let service = JwtService::new(config);

        // Create a token with expired timestamp directly
        let now = Utc::now();
        let expired_time = now - Duration::seconds(3600); // 1 hour ago

        let claims = Claims {
            sub: "user".to_string(),
            name: None,
            exp: expired_time.timestamp(),
            iat: expired_time.timestamp(),
            iss: service.config.issuer.clone(),
        };

        let header = Header::new(Algorithm::HS256);
        let token = encode(&header, &claims, &service.encoding_key).unwrap();

        let result = service.validate_token(&token);
        assert!(result.is_err());
        if let Err(HttpError::AuthenticationFailed(msg)) = result {
            assert!(msg.to_lowercase().contains("expired"));
        } else {
            panic!("Expected authentication failed error");
        }
    }

    #[test]
    fn test_extract_bearer_token() {
        let config = JwtConfig::new("test-secret".to_string(), 24, "test-issuer".to_string());
        let service = JwtService::new(config);

        assert_eq!(
            service.extract_bearer_token("Bearer abc123").unwrap(),
            "abc123"
        );

        assert!(service.extract_bearer_token("Basic abc123").is_err());
        assert!(service.extract_bearer_token("abc123").is_err());
    }
}
