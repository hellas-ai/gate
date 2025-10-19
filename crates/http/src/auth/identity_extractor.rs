//! Extract identity from HTTP headers

use axum::http::HeaderMap;
use http::HeaderName;
use http::header::AUTHORIZATION;
const X_API_KEY: HeaderName = HeaderName::from_static("x-api-key");
const X_USER_ID: HeaderName = HeaderName::from_static("x-user-id");
const X_ORG_ID: HeaderName = HeaderName::from_static("x-org-id");
use gate_core::access::SubjectIdentity;
use gate_core::router::connector::RouterIdentityContext;
use sha2::Digest;

/// Extract identity from request headers
pub fn extract_identity(headers: &HeaderMap) -> SubjectIdentity<RouterIdentityContext> {
    // Check for Authorization header
    if let Some(auth_header) = headers.get(AUTHORIZATION)
        && let Ok(auth_str) = auth_header.to_str()
    {
        return parse_authorization_header(auth_str);
    }

    // Check for X-API-Key header
    if let Some(api_key) = headers.get(X_API_KEY)
        && let Ok(key_str) = api_key.to_str()
    {
        return create_api_key_identity(key_str);
    }

    // Check for X-User-ID header (for internal services)
    if let Some(user_id) = headers.get(X_USER_ID)
        && let Ok(user_str) = user_id.to_str()
    {
        let org_id = headers
            .get(X_ORG_ID)
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        return SubjectIdentity::new(
            user_str,
            "internal",
            RouterIdentityContext {
                org_id,
                user_id: Some(user_str.to_string()),
                api_key_hash: None,
            },
        );
    }

    // Default to anonymous
    SubjectIdentity::new("anonymous", "http", RouterIdentityContext::default())
}

fn parse_authorization_header(auth_str: &str) -> SubjectIdentity<RouterIdentityContext> {
    let parts: Vec<&str> = auth_str.splitn(2, ' ').collect();

    match parts.as_slice() {
        ["Bearer", token] => parse_bearer_token(token),
        ["Basic", credentials] => parse_basic_auth(credentials),
        ["ApiKey", key] => create_api_key_identity(key),
        _ => {
            // Unknown auth type
            SubjectIdentity::new("unknown", "http", RouterIdentityContext::default())
        }
    }
}

fn parse_bearer_token(token: &str) -> SubjectIdentity<RouterIdentityContext> {
    // In a real implementation, we would validate the JWT token
    // and extract claims. For now, we'll create a simple identity

    // Hash the token for the identity ID (in production, validate and extract sub claim)
    use sha2::Sha256;
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let token_hash = format!("{:x}", hasher.finalize());

    SubjectIdentity::new(
        &token_hash[..8], // Use first 8 chars of hash as ID
        "bearer",
        RouterIdentityContext {
            org_id: None,  // Would be extracted from JWT claims
            user_id: None, // Would be extracted from JWT claims
            api_key_hash: None,
        },
    )
}

fn parse_basic_auth(credentials: &str) -> SubjectIdentity<RouterIdentityContext> {
    // Decode base64
    use base64::{Engine as _, engine::general_purpose};
    if let Ok(decoded) = general_purpose::STANDARD.decode(credentials)
        && let Ok(decoded_str) = String::from_utf8(decoded)
    {
        let parts: Vec<&str> = decoded_str.splitn(2, ':').collect();
        if parts.len() == 2 {
            let username = parts[0];
            // In production, would validate password

            return SubjectIdentity::new(
                username,
                "basic",
                RouterIdentityContext {
                    org_id: None,
                    user_id: Some(username.to_string()),
                    api_key_hash: None,
                },
            );
        }
    }

    SubjectIdentity::new("invalid_basic", "http", RouterIdentityContext::default())
}

fn create_api_key_identity(api_key: &str) -> SubjectIdentity<RouterIdentityContext> {
    use sha2::{Digest, Sha256};

    // Hash the API key
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let hash_result = hasher.finalize();
    let api_key_hash = format!("{hash_result:x}");

    // Use first 8 chars of hash as identity ID
    SubjectIdentity::new(
        &api_key_hash[..8],
        "api_key",
        RouterIdentityContext {
            org_id: None,  // Would be looked up from database
            user_id: None, // Would be looked up from database
            api_key_hash: Some(api_key_hash.clone()),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_anonymous() {
        let headers = HeaderMap::new();
        let identity = extract_identity(&headers);
        assert_eq!(identity.id, "anonymous");
        assert_eq!(identity.source, "http");
    }

    #[test]
    fn test_extract_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "test-key-123".parse().unwrap());

        let identity = extract_identity(&headers);
        assert_eq!(identity.source, "api_key");
        assert!(identity.context.api_key_hash.is_some());
    }

    #[test]
    fn test_extract_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer some-jwt-token".parse().unwrap());

        let identity = extract_identity(&headers);
        assert_eq!(identity.source, "bearer");
    }

    #[test]
    fn test_extract_basic_auth() {
        let mut headers = HeaderMap::new();
        // "user:pass" in base64
        headers.insert("authorization", "Basic dXNlcjpwYXNz".parse().unwrap());

        let identity = extract_identity(&headers);
        assert_eq!(identity.source, "basic");
        assert_eq!(identity.id, "user");
        assert_eq!(identity.context.user_id, Some("user".to_string()));
    }

    #[test]
    fn test_extract_internal_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-user-id", "user123".parse().unwrap());
        headers.insert("x-org-id", "org456".parse().unwrap());

        let identity = extract_identity(&headers);
        assert_eq!(identity.source, "internal");
        assert_eq!(identity.id, "user123");
        assert_eq!(identity.context.user_id, Some("user123".to_string()));
        assert_eq!(identity.context.org_id, Some("org456".to_string()));
    }
}
