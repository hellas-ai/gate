//! Server configuration

use std::path::PathBuf;

use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use serde_json::json;

fn default_host() -> String {
    "localhost".to_string()
}

fn default_port() -> u16 {
    31145
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_rp_id() -> String {
    default_host()
}

fn default_rp_name() -> String {
    "Gate Self-Hosted".to_string()
}

fn default_rp_origin() -> String {
    format!("http://{}:{}", default_host(), default_port())
}

fn default_session_timeout() -> u64 {
    86400 // 24 hours
}

fn default_timeout() -> u64 {
    30 // 30 seconds
}

fn default_jwt_issuer() -> String {
    "gate-daemon".to_string()
}

fn generate_random_secret() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let secret: String = (0..32)
        .map(|_| rng.sample(rand::distributions::Alphanumeric) as char)
        .collect();
    secret
}

/// Default JWT secret, read from environment variable or generated if not set
fn default_jwt_secret() -> String {
    std::env::var("JWT_SECRET").unwrap_or_else(|_| generate_random_secret())
}

fn default_jwt_expiration_hours() -> u64 {
    24 // 24 hours
}

fn default_heartbeat_interval() -> u64 {
    30 // 30 seconds
}

fn default_max_reconnect_attempts() -> u32 {
    10
}

fn default_reconnect_backoff() -> u64 {
    5 // 5 seconds
}

fn default_auto_renew_days() -> u32 {
    30 // 30 days before expiry
}

fn default_tlsforward_max_connections() -> usize {
    1000
}

fn default_local_inference() -> Option<LocalInferenceConfig> {
    Some(LocalInferenceConfig {
        enabled: true,
        max_concurrent_inferences: 1,
        default_temperature: 0.7,
        default_max_tokens: 1024,
        models: vec![],
    })
}

fn default_tlsforward_addresses() -> Vec<String> {
    vec![
        "3dbefb2e3d56c7e32586d9a82167a8a5151f3e0f4b40b7c3d145b9060dde2f14@213.239.212.173:31145"
            .to_string(),
    ]
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Server settings
    #[serde(default)]
    pub server: ServerConfig,
    /// Authentication settings
    #[serde(default)]
    pub auth: AuthConfig,
    /// Provider configurations
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    /// Relay configuration
    #[serde(default)]
    pub tlsforward: TlsForwardConfig,
    /// Let's Encrypt configuration
    #[serde(default)]
    pub letsencrypt: LetsEncryptConfig,
    /// Local inference configuration
    #[serde(default = "default_local_inference")]
    pub local_inference: Option<LocalInferenceConfig>,
}

impl Default for Settings {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("Default settings should always be valid")
    }
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Host to bind to
    #[serde(default = "default_host")]
    pub host: String,
    /// Port to bind to
    #[serde(default = "default_port")]
    pub port: u16,
    /// CORS allowed origins
    #[serde(default)]
    pub cors_origins: Vec<String>,
    /// Prometheus metrics endpoint port (if enabled)
    #[serde(default)]
    pub metrics_port: Option<u16>,
    /// Allow localhost clients to bypass auth (effective only when host is loopback)
    #[serde(default = "default_true")]
    pub allow_local_bypass: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("Default settings should always be valid")
    }
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// WebAuthn configuration
    #[serde(default)]
    pub webauthn: WebAuthnConfig,
    /// JWT configuration
    #[serde(default)]
    pub jwt: JwtConfig,
    /// Registration configuration
    #[serde(default)]
    pub registration: RegistrationConfig,
    /// Provider API key passthrough (Anthropic/OpenAI) for inference routes
    #[serde(default)]
    pub provider_passthrough: ProviderPassthroughConfig,
}

impl Default for AuthConfig {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("Default settings should always be valid")
    }
}

/// Provider passthrough configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderPassthroughConfig {
    /// Enable passthrough of provider API keys from clients
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Restrict passthrough to loopback peers only
    #[serde(default = "default_true")]
    pub loopback_only: bool,
    /// Allowed HTTP paths for passthrough (exact match)
    #[serde(default = "default_passthrough_paths")]
    pub allowed_paths: Vec<String>,
}

fn default_passthrough_paths() -> Vec<String> {
    vec![
        "/v1/messages".to_string(),         // Anthropic
        "/v1/chat/completions".to_string(), // OpenAI Chat
        "/v1/responses".to_string(),        // OpenAI Responses
        "/v1/completions".to_string(),      // OpenAI Completions (legacy)
    ]
}

impl Default for ProviderPassthroughConfig {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("Default settings should always be valid")
    }
}

/// Provider type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    Anthropic,
    OpenAI,
    Custom,
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderType::Anthropic => write!(f, "Anthropic"),
            ProviderType::OpenAI => write!(f, "OpenAI"),
            ProviderType::Custom => write!(f, "Custom"),
        }
    }
}

/// Provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Name identifier for this provider
    pub name: String,
    /// Provider type
    pub provider: ProviderType,
    /// Base URL for the upstream API
    pub base_url: String,
    /// API key for authentication (can be set via env var)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    /// List of supported models (populated on startup)
    #[serde(default, skip_serializing)]
    pub models: Vec<String>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("Default settings should always be valid")
    }
}

/// WebAuthn configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnConfig {
    /// Relying Party ID (usually domain name)
    #[serde(default = "default_rp_id")]
    pub rp_id: String,
    /// Relying Party Name (display name)
    #[serde(default = "default_rp_name")]
    pub rp_name: String,
    /// Relying Party Origin (full URL)
    #[serde(default = "default_rp_origin")]
    pub rp_origin: String,
    /// Additional allowed origins
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// Allow relay origins automatically (*.hellas.ai domains)
    #[serde(default = "default_true")]
    pub allow_tlsforward_origins: bool,
    /// Allow subdomains of configured origins
    #[serde(default = "default_true")]
    pub allow_subdomains: bool,
    /// Require user verification
    #[serde(default = "default_false")]
    pub require_user_verification: bool,
    /// Session timeout in seconds
    #[serde(default = "default_session_timeout")]
    pub session_timeout_seconds: u64,
}

impl Default for WebAuthnConfig {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("Default settings should always be valid")
    }
}

/// JWT configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtConfig {
    /// JWT issuer
    #[serde(default = "default_jwt_issuer")]
    pub issuer: String,
    /// JWT secret (read from JWT_SECRET env var or generate)
    #[serde(default = "default_jwt_secret")]
    pub secret: String,
    /// Token expiration in hours
    #[serde(default = "default_jwt_expiration_hours")]
    pub expiration_hours: u64,
}

impl Default for JwtConfig {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("Default settings should always be valid")
    }
}

/// Registration configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationConfig {
    /// Allow open registration after bootstrap
    #[serde(default = "default_false")]
    pub allow_open_registration: bool,
}

impl Default for RegistrationConfig {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("Default settings should always be valid")
    }
}

/// TLS forward configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsForwardConfig {
    /// Enable TLS forward functionality
    #[serde(default = "default_false")]
    pub enabled: bool,
    /// List of TLS forward server addresses (NodeAddr format)
    #[serde(default = "default_tlsforward_addresses")]
    pub tlsforward_addresses: Vec<String>,
    /// Maximum concurrent TLS connections
    #[serde(default = "default_tlsforward_max_connections")]
    pub max_connections: usize,
    /// Path to store the secret key for persistent node ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_key_path: Option<String>,
    /// Heartbeat interval in seconds
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval: u64,
    /// Auto-reconnect on disconnect
    #[serde(default = "default_true")]
    pub auto_reconnect: bool,
    /// Maximum reconnection attempts
    #[serde(default = "default_max_reconnect_attempts")]
    pub max_reconnect_attempts: u32,
    /// Reconnection backoff in seconds
    #[serde(default = "default_reconnect_backoff")]
    pub reconnect_backoff: u64,
}

impl Default for TlsForwardConfig {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("Default settings should always be valid")
    }
}

/// Local inference configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalInferenceConfig {
    /// Whether local inference is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Maximum concurrent inference requests
    #[serde(default = "default_max_concurrent_inferences")]
    pub max_concurrent_inferences: usize,
    /// Default temperature for inference when not specified
    #[serde(default = "default_temperature")]
    pub default_temperature: f32,
    /// Default max tokens for inference when not specified
    #[serde(default = "default_max_tokens")]
    pub default_max_tokens: u32,
    /// List of available models for local inference
    #[serde(default)]
    pub models: Vec<String>,
}

impl Default for LocalInferenceConfig {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("Default settings should always be valid")
    }
}

fn default_max_concurrent_inferences() -> usize {
    4
}

fn default_temperature() -> f32 {
    0.7
}

fn default_max_tokens() -> u32 {
    2048
}

/// Let's Encrypt configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LetsEncryptConfig {
    /// Enable Let's Encrypt certificate management
    #[serde(default = "default_false")]
    pub enabled: bool,
    /// Email address for ACME account
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Use staging environment for testing
    #[serde(default = "default_false")]
    pub staging: bool,
    /// Domains to request certificates for
    #[serde(default)]
    pub domains: Vec<String>,
    /// Auto-renew certificates before expiry (days)
    #[serde(default = "default_auto_renew_days")]
    pub auto_renew_days: u32,
}

impl Default for LetsEncryptConfig {
    fn default() -> Self {
        serde_json::from_value(json!({})).expect("Default settings should always be valid")
    }
}

impl Settings {
    /// Load settings from a specific config file
    pub fn load_from_file(path: impl Into<PathBuf>) -> Result<Self, ConfigError> {
        let mut builder = Config::builder();

        // Start with defaults
        builder = builder.add_source(Config::try_from(&Settings::default())?);

        // Add the specific config file
        builder = builder.add_source(File::from(path.into()).required(true));

        // Add environment variables with GATE_ prefix (can override file settings)
        builder = builder.add_source(
            Environment::with_prefix("GATE")
                .separator("__")
                .try_parsing(true),
        );

        let config = builder.build()?;
        config.try_deserialize()
    }

    pub async fn save_to_file(&self, path: impl Into<PathBuf>) -> Result<(), std::io::Error> {
        let config_str = serde_json::to_string_pretty(self)?;
        std::fs::write(path.into(), config_str)?;
        Ok(())
    }
}
