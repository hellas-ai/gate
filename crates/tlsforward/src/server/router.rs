//! HTTP API router for relay control and DNS challenge management

use axum::{
    Router,
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{delete, get, post},
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::dns_challenge::{DnsChallenge, DnsChallengeManager};
use super::registry::ProxyRegistry;
use crate::common::{types::TlsForwardInfo, *};
use iroh::NodeId;

/// API state combining DNS manager and proxy registry
#[derive(Clone)]
pub struct ApiState {
    pub dns_manager: Arc<DnsChallengeManager>,
    pub registry: Arc<ProxyRegistry>,
    pub domain_suffix: String,
    /// The TLS forward server's own node ID
    pub tlsforward_node_id: NodeId,
    ///// The TLS forward server's own node address
    // pub tlsforward_node_addr: iroh::NodeAddr,
}

/// Create the router for TLS forward APIs
pub fn create_api_router() -> Router<ApiState> {
    Router::new()
        // ACME endpoints
        .route("/acme/challenge", post(create_challenge))
        .route("/acme/challenge/{id}/status", get(get_challenge_status))
        .route("/acme/challenge/{id}", delete(delete_challenge))
        // TLS forward control endpoints
        .route("/register", post(handle_register))
        .route("/unregister", delete(handle_unregister))
        .route("/status", get(handle_status))
        .route("/ping", post(handle_ping))
        .route("/nodes", get(list_nodes))
}

/// Create a new DNS challenge
#[instrument(
    name = "create_dns_challenge",
    skip(state),
    fields(
        node_id = %node_id,
        domain = %req.domain,
        challenge_type = %req.challenge
    )
)]
async fn create_challenge(
    State(state): State<ApiState>,
    Extension(node_id): Extension<NodeId>,
    Json(req): Json<CreateChallengeRequest>,
) -> impl IntoResponse {
    let manager = &state.dns_manager;
    info!(
        "Creating DNS challenge for domain: {} with value: {} from node: {}",
        req.domain, req.value, node_id
    );
    debug!(
        "Challenge request details - domain: {}, challenge: {}, value: {}",
        req.domain, req.challenge, req.value
    );

    // Validate domain format (should match our pattern)
    if !manager.validate_domain(&req.domain) {
        error!("Invalid domain format: {}", req.domain);
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "INVALID_DOMAIN",
                format!("Invalid domain format: {}", req.domain),
            )),
        )
            .into_response();
    }

    // Verify that the requesting node owns this domain
    let expected_short_hash = node_id.fmt_short();
    let domain_prefix = req.domain.split('.').next().unwrap_or("");

    if domain_prefix != expected_short_hash {
        error!(
            "Node {} attempted to create challenge for domain {} which it doesn't own",
            node_id, req.domain
        );
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "DOMAIN_NOT_OWNED",
                "You can only create challenges for domains assigned to your node",
            )),
        )
            .into_response();
    }

    // Create the challenge
    let challenge = DnsChallenge {
        domain: req.domain,
        challenge: req.challenge,
        value: req.value,
    };

    match manager.create_challenge(challenge, node_id).await {
        Ok(id) => {
            info!("DNS challenge created successfully with ID: {}", id);
            (
                StatusCode::OK,
                Json(CreateChallengeResponse {
                    id,
                    status: ChallengeStatus::Pending,
                }),
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to create DNS challenge: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(
                    "DNS_CHALLENGE_FAILED",
                    format!("Failed to create DNS challenge: {e}"),
                )),
            )
                .into_response()
        }
    }
}

/// Get the status of a DNS challenge
async fn get_challenge_status(
    State(state): State<ApiState>,
    Extension(node_id): Extension<NodeId>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    debug!("Checking status for challenge: {}", id);

    match state.dns_manager.get_challenge_status(&id, &node_id).await {
        Ok((status, checks)) => (
            StatusCode::OK,
            Json(ChallengeStatusResponse { id, status, checks }),
        )
            .into_response(),
        Err(e) => {
            error!("Failed to get challenge status: {}", e);
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new(
                    "CHALLENGE_NOT_FOUND",
                    format!("Challenge not found: {id}"),
                )),
            )
                .into_response()
        }
    }
}

/// Delete a DNS challenge
async fn delete_challenge(
    State(state): State<ApiState>,
    Extension(node_id): Extension<NodeId>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let manager = &state.dns_manager;
    info!("Deleting challenge: {}", id);

    match manager.delete_challenge(&id, &node_id).await {
        Ok(_) => (
            StatusCode::OK,
            Json(DeleteChallengeResponse {
                id,
                status: "deleted".to_string(),
            }),
        )
            .into_response(),
        Err(e) => {
            error!("Failed to delete challenge: {}", e);
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new(
                    "CHALLENGE_NOT_FOUND",
                    format!("Challenge not found: {id}"),
                )),
            )
                .into_response()
        }
    }
}

// Relay control handlers

/// Handle registration request
async fn handle_register(
    State(state): State<ApiState>,
    Extension(node_id): Extension<NodeId>,
    _: Json<RegistrationRequest>,
) -> impl IntoResponse {
    // Generate domain
    let domain = format!("{}.{}", node_id.fmt_short(), state.domain_suffix);

    match state.registry.register(node_id).await {
        Ok(_) => {
            info!("Registered node {} with domain {}", node_id, domain);
            debug!(
                "Registry now contains {} nodes",
                state.registry.list_all().await.len()
            );

            // Build TLS forward info with the TLS forward server's information
            let tlsforward_info = TlsForwardInfo {
                node_id: state.tlsforward_node_id,
                domain_suffix: state.domain_suffix.clone(),
            };

            (
                StatusCode::OK,
                Json(RegistrationResponse {
                    domain,
                    tlsforward_info,
                }),
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to register node {}: {}", node_id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(
                    "REGISTRATION_FAILED",
                    format!("Failed to register node: {e}"),
                )),
            )
                .into_response()
        }
    }
}

/// Handle unregister request
async fn handle_unregister(
    State(state): State<ApiState>,
    Extension(node_id): Extension<NodeId>,
) -> StatusCode {
    match state.registry.unregister(&node_id).await {
        Ok(_) => {
            info!("Unregistered node {}", node_id);
            StatusCode::OK
        }
        Err(e) => {
            error!("Failed to unregister node {}: {}", node_id, e);
            StatusCode::NOT_FOUND
        }
    }
}

/// Handle status request
async fn handle_status(
    State(state): State<ApiState>,
    Extension(node_id): Extension<NodeId>,
) -> Json<StatusResponse> {
    let is_registered = state.registry.lookup_by_node_id(&node_id).await.is_ok();

    let domain = if is_registered {
        Some(format!("{}.{}", node_id.fmt_short(), state.domain_suffix))
    } else {
        None
    };

    Json(StatusResponse {
        registered: is_registered,
        domain,
        active_connections: 0, // TODO: Track this
        uptime_seconds: 0,     // TODO: Track this
    })
}

/// Handle ping request
#[instrument(name = "handle_ping", skip_all)]
async fn handle_ping(
    State(state): State<ApiState>,
    Extension(node_id): Extension<NodeId>,
) -> impl IntoResponse {
    // Update ping information for the node
    match state.registry.update_ping(&node_id, None).await {
        Ok(_) => {
            debug!("Updated ping for node {}", node_id);
        }
        Err(e) => {
            // If node is not registered, this is not an error - just log at debug level
            // Nodes may ping before registering
            debug!("Node {} not registered yet, ignoring ping: {}", node_id, e);
        }
    }

    Json(EmptyResponse {})
}

/// List all connected nodes
#[instrument(name = "list_nodes", skip_all)]
async fn list_nodes(State(state): State<ApiState>) -> Json<ListNodesResponse> {
    let all_nodes = state.registry.list_all().await;
    let mut nodes = Vec::with_capacity(all_nodes.len());

    for (short_hash, entry) in all_nodes {
        let domain = format!("{}.{}", short_hash, state.domain_suffix);

        // Calculate uptime
        let uptime_seconds = entry.connected_at.elapsed().as_secs();

        // Convert Instant to SystemTime for ISO 8601 formatting
        let connected_at = SystemTime::now()
            .checked_sub(entry.connected_at.elapsed())
            .unwrap_or(UNIX_EPOCH);
        let connected_at_dt: DateTime<Utc> = connected_at.into();

        let last_ping = SystemTime::now()
            .checked_sub(entry.last_ping.elapsed())
            .unwrap_or(UNIX_EPOCH);
        let last_ping_dt: DateTime<Utc> = last_ping.into();

        nodes.push(ConnectedNode {
            node_id: entry.node_id.to_string(),
            domain,
            connected_at: connected_at_dt.to_rfc3339(),
            uptime_seconds,
            latency_ms: entry.latency_ms,
            last_ping: last_ping_dt.to_rfc3339(),
        });
    }

    let total = nodes.len();

    Json(ListNodesResponse { nodes, total })
}
