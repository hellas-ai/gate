//! TLS forward server configuration

use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

/// TLS forward server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsForwardConfig {
    /// Server settings
    pub server: ServerConfig,
    /// P2P settings
    pub p2p: P2pConfig,
    /// HTTPS proxy settings
    pub https_proxy: HttpsProxyConfig,
    /// DNS settings
    #[serde(default)]
    pub dns: DnsConfig,
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Log level
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// Metrics endpoint
    #[serde(default)]
    pub metrics_addr: Option<SocketAddr>,
}

/// P2P configuration
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct P2pConfig {
    /// P2P bind address
    #[serde(default)]
    pub bind_addrs: Vec<SocketAddr>,
    /// Secret key path (if not provided, generates new)
    #[serde(default)]
    pub secret_key_path: Option<String>,
    /// Enable local network discovery
    #[serde(default = "default_true")]
    pub enable_discovery: bool,
}

/// HTTPS proxy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpsProxyConfig {
    /// Bind address for HTTPS proxy
    #[serde(default = "default_https_addr")]
    pub bind_addr: SocketAddr,
    /// Domain suffix for relay addresses
    #[serde(default = "default_domain_suffix")]
    pub domain_suffix: String,
    /// Maximum concurrent connections
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    /// Connection timeout in seconds
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout_secs: u64,
}

/// DNS configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DnsConfig {
    /// Enable DNS management
    #[serde(default)]
    pub enabled: bool,
    /// DNS provider
    #[serde(default)]
    pub provider: DnsProvider,
    /// Cloudflare configuration
    #[serde(default)]
    pub cloudflare: CloudflareConfig,
}

/// DNS provider
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DnsProvider {
    #[default]
    None,
    Cloudflare,
}

/// Cloudflare configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CloudflareConfig {
    /// Zone ID
    pub zone_id: Option<String>,
    /// API token
    pub api_token: Option<String>,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_true() -> bool {
    true
}

fn default_https_addr() -> SocketAddr {
    ([0, 0, 0, 0], 443).into()
}

fn default_domain_suffix() -> String {
    "private.hellas.ai".to_string()
}

fn default_max_connections() -> usize {
    1000
}

fn default_connection_timeout() -> u64 {
    10
}

impl Default for TlsForwardConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                log_level: default_log_level(),
                metrics_addr: None,
            },
            p2p: P2pConfig::default(),
            https_proxy: HttpsProxyConfig {
                bind_addr: default_https_addr(),
                domain_suffix: default_domain_suffix(),
                max_connections: default_max_connections(),
                connection_timeout_secs: default_connection_timeout(),
            },
            dns: DnsConfig::default(),
        }
    }
}

impl TlsForwardConfig {
    /// Load configuration from environment and files
    pub fn load() -> Result<Self, ConfigError> {
        let mut builder = Config::builder();

        // Try to find config files in common locations
        let config_paths = [
            "relay.toml",
            "config/relay.toml",
            "/etc/gate/relay.toml",
            "gate.toml", // Also check main gate config
            "config/gate.toml",
            "/etc/gate/gate.toml",
        ];

        for path in &config_paths {
            if Path::new(path).exists() {
                builder = builder.add_source(File::with_name(path).required(false));
            }
        }

        // Add environment variables with RELAY_ prefix
        builder = builder.add_source(
            Environment::with_prefix("RELAY")
                .separator("__")
                .try_parsing(true),
        );

        let config = builder.build()?;
        config.try_deserialize()
    }

    /// Load configuration from a specific config file
    pub fn load_from_file(path: &str) -> Result<Self, ConfigError> {
        let mut builder = Config::builder();

        // Start with defaults
        builder = builder.add_source(Config::try_from(&TlsForwardConfig::default())?);

        // Add the specific config file
        builder = builder.add_source(File::with_name(path));

        // Add environment variables with RELAY_ prefix (can override file settings)
        builder = builder.add_source(
            Environment::with_prefix("RELAY")
                .separator("__")
                .try_parsing(true),
        );

        let config = builder.build()?;
        config.try_deserialize()
    }
}

/// Timeout configuration for various proxy operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyTimeouts {
    /// Timeout for reading SNI from incoming TLS connection
    #[serde(default = "default_sni_read_timeout")]
    pub sni_read: Duration,

    /// Timeout for establishing P2P connection to target node
    #[serde(default = "default_connect_timeout")]
    pub connect: Duration,

    /// Timeout for copying data between streams
    #[serde(default = "default_stream_copy_timeout")]
    pub stream_copy: Duration,

    /// Timeout for DNS operations (if using Cloudflare)
    #[serde(default = "default_dns_operation_timeout")]
    pub dns_operation: Duration,

    /// Idle timeout for connections
    #[serde(default = "default_idle_timeout")]
    pub idle: Duration,
}

impl Default for ProxyTimeouts {
    fn default() -> Self {
        Self {
            sni_read: default_sni_read_timeout(),
            connect: default_connect_timeout(),
            stream_copy: default_stream_copy_timeout(),
            dns_operation: default_dns_operation_timeout(),
            idle: default_idle_timeout(),
        }
    }
}

impl ProxyTimeouts {
    /// Create a new timeout configuration with all values set to the same duration
    pub fn all(duration: Duration) -> Self {
        Self {
            sni_read: duration,
            connect: duration,
            stream_copy: duration,
            dns_operation: duration,
            idle: duration,
        }
    }

    /// Create timeout configuration suitable for testing
    pub fn for_testing() -> Self {
        Self {
            sni_read: Duration::from_secs(1),
            connect: Duration::from_secs(5),
            stream_copy: Duration::from_secs(60),
            dns_operation: Duration::from_secs(10),
            idle: Duration::from_secs(120),
        }
    }
}

fn default_sni_read_timeout() -> Duration {
    Duration::from_secs(5)
}

fn default_connect_timeout() -> Duration {
    Duration::from_secs(10)
}

fn default_stream_copy_timeout() -> Duration {
    Duration::from_secs(300) // 5 minutes
}

fn default_dns_operation_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_idle_timeout() -> Duration {
    Duration::from_secs(600) // 10 minutes
}
