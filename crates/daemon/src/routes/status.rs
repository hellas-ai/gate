//! Public status endpoint for daemon information

use axum::{extract::State, response::Json};
use gate_http::state::AppState;
use serde::{Deserialize, Serialize};
use utoipa_axum::{router::OpenApiRouter, routes};

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DaemonStatusResponse {
    pub node_id: String,
    pub version: String,
    pub is_initialized: bool,
    pub listen_addresses: Vec<String>,
    pub database_path: Option<String>,
    pub config_path: Option<String>,
    pub uptime_seconds: u64,
    pub hostname: String,
}

/// Get public daemon status information
#[utoipa::path(
    get,
    path = "/api/status",
    responses(
        (status = 200, description = "Daemon status information", body = DaemonStatusResponse),
    ),
    tag = "status"
)]
pub async fn get_daemon_status(
    State(state): State<AppState<crate::ServerState>>,
) -> Json<DaemonStatusResponse> {
    let settings = state.data.settings.as_ref();
    let db = state.state_backend.as_ref();

    // Get node ID - for now just use a placeholder
    let node_id = "not-initialized".to_string();

    // Check if initialized by checking for users
    let is_initialized = match db.list_users().await {
        Ok(users) => !users.is_empty(),
        Err(_) => false,
    };

    // Get listen addresses
    let listen_addresses = vec![format!("{}:{}", settings.server.host, settings.server.port)];

    // For now, just use the main listen address

    // Get database path
    let database_path = std::env::var("DATABASE_URL")
        .ok()
        .filter(|url| !url.contains(":memory:"))
        .map(|url| url.replace("sqlite://", ""));

    // Get config path
    let config_path = std::env::var("GATE_CONFIG_PATH").ok();

    // Calculate uptime (this is a simple approximation)
    let uptime_seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Get hostname
    let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string());

    Json(DaemonStatusResponse {
        node_id,
        version: env!("CARGO_PKG_VERSION").to_string(),
        is_initialized,
        listen_addresses,
        database_path,
        config_path,
        uptime_seconds,
        hostname,
    })
}

/// Add status routes to the router
pub fn add_routes(
    router: OpenApiRouter<AppState<crate::ServerState>>,
) -> OpenApiRouter<AppState<crate::ServerState>> {
    router.routes(routes!(get_daemon_status))
}
