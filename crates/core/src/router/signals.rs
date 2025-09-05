use http::header::{AUTHORIZATION, HeaderName};

/// Return Anthropic API key from headers, if present (either `x-api-key` or `Authorization: Bearer sk-ant-*`).
pub fn anthropic_key_from(headers: &http::HeaderMap) -> Option<&str> {
    let x_api_key = HeaderName::from_static("x-api-key");
    if let Some(val) = headers.get(&x_api_key).and_then(|v| v.to_str().ok())
        && val.starts_with("sk-ant-")
    {
        return Some(val);
    }
    if let Some(auth) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok())
        && let Some(token) = auth.strip_prefix("Bearer ")
        && token.starts_with("sk-ant-")
    {
        return Some(token);
    }
    None
}

/// Return OpenAI bearer token from headers, if present (`Authorization: Bearer <token>`).
pub fn openai_bearer_from(headers: &http::HeaderMap) -> Option<&str> {
    if let Some(auth) = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok())
        && let Some(token) = auth.strip_prefix("Bearer ")
        && !token.is_empty()
    {
        return Some(token);
    }
    None
}

/// Whether headers contain Anthropic-specific signals (version header or key).
pub fn has_anthropic_signal(headers: &http::HeaderMap) -> bool {
    let anth_version = HeaderName::from_static("anthropic-version");
    headers.contains_key(&anth_version) || anthropic_key_from(headers).is_some()
}

/// Whether headers contain OpenAI-specific signals (beta header or Authorization Bearer).
pub fn has_openai_signal(headers: &http::HeaderMap) -> bool {
    let openai_beta = HeaderName::from_static("openai-beta");
    headers.contains_key(&openai_beta) || openai_bearer_from(headers).is_some()
}
