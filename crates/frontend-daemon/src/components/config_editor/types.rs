use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GateConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub tlsforward: TlsForwardConfig,
    #[serde(default)]
    pub letsencrypt: LetsEncryptConfig,
    #[serde(default)]
    pub local_inference: Option<LocalInferenceConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub metrics_port: Option<u16>,
    #[serde(default = "default_true")]
    pub allow_local_bypass: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            metrics_port: None,
            allow_local_bypass: default_true(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AuthConfig {
    #[serde(default)]
    pub webauthn: WebAuthnConfig,
    #[serde(default)]
    pub jwt: JwtConfig,
    #[serde(default)]
    pub registration: RegistrationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebAuthnConfig {
    #[serde(default = "default_rp_id")]
    pub rp_id: String,
    #[serde(default = "default_rp_name")]
    pub rp_name: String,
    #[serde(default = "default_rp_origin")]
    pub rp_origin: String,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}

impl Default for WebAuthnConfig {
    fn default() -> Self {
        Self {
            rp_id: default_rp_id(),
            rp_name: default_rp_name(),
            rp_origin: default_rp_origin(),
            allowed_origins: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JwtConfig {
    #[serde(default = "default_jwt_issuer")]
    pub issuer: String,
    #[serde(default = "default_jwt_secret")]
    pub secret: String,
    #[serde(default = "default_jwt_expiration_hours")]
    pub expiration_hours: u64,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            issuer: default_jwt_issuer(),
            secret: default_jwt_secret(),
            expiration_hours: default_jwt_expiration_hours(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RegistrationConfig {
    #[serde(default)]
    pub allow_open_registration: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderConfig {
    pub name: String,
    pub provider: String,
    pub base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TlsForwardConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_tlsforward_addresses")]
    pub tlsforward_addresses: Vec<String>,
    #[serde(default = "default_tlsforward_max_connections")]
    pub max_connections: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_key_path: Option<String>,
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval: u64,
}

impl Default for TlsForwardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            tlsforward_addresses: default_tlsforward_addresses(),
            max_connections: default_tlsforward_max_connections(),
            secret_key_path: None,
            heartbeat_interval: default_heartbeat_interval(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LetsEncryptConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default)]
    pub staging: bool,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default = "default_auto_renew_days")]
    pub auto_renew_days: u32,
}

impl Default for LetsEncryptConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            email: None,
            staging: false,
            domains: Vec::new(),
            auto_renew_days: default_auto_renew_days(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalInferenceConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_concurrent_inferences")]
    pub max_concurrent_inferences: usize,
    #[serde(default = "default_temperature")]
    pub default_temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub default_max_tokens: u32,
}

impl Default for LocalInferenceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent_inferences: default_max_concurrent_inferences(),
            default_temperature: default_temperature(),
            default_max_tokens: default_max_tokens(),
        }
    }
}

// Default value functions
fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    31145
}

fn default_true() -> bool {
    true
}

fn default_rp_id() -> String {
    "localhost".to_string()
}

fn default_rp_name() -> String {
    "Gate Self-Hosted".to_string()
}

fn default_rp_origin() -> String {
    format!("http://localhost:{}", default_port())
}

fn default_jwt_issuer() -> String {
    "gate-daemon".to_string()
}

fn default_jwt_secret() -> String {
    "change-me-in-production".to_string()
}

fn default_jwt_expiration_hours() -> u64 {
    24
}

pub fn default_timeout() -> u64 {
    30
}

fn default_tlsforward_addresses() -> Vec<String> {
    vec![
        "3dbefb2e3d56c7e32586d9a82167a8a5151f3e0f4b40b7c3d145b9060dde2f14@213.239.212.173:31145"
            .to_string(),
    ]
}

fn default_tlsforward_max_connections() -> usize {
    1000
}

fn default_heartbeat_interval() -> u64 {
    30
}

fn default_auto_renew_days() -> u32 {
    30
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
