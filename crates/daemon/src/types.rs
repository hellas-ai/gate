use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DaemonStatus {
    pub running: bool,
    pub listen_address: String,
    pub upstream_count: usize,
    pub user_count: usize,
    pub tlsforward_enabled: bool,
    pub tlsforward_status: TlsForwardStatus,
    pub needs_bootstrap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub enum TlsForwardStatus {
    Disabled,
    Disconnected,
    Connecting,
    Connected { domain: String },
    Error(String),
}

/// Response for bootstrap status endpoint
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BootstrapStatusResponse {
    /// Whether the system needs bootstrap (no users exist)
    pub needs_bootstrap: bool,
    /// Whether bootstrap process is complete
    pub is_complete: bool,
    /// Human-readable status message
    pub message: String,
}

/// Response for daemon runtime configuration
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DaemonRuntimeConfigResponse {
    /// Server address the daemon is listening on
    pub server_address: String,
    /// Whether TLS forward is enabled
    pub tlsforward_enabled: bool,
}
