use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::Path;
use std::sync::Arc;

use hellas_rpc::pb::host::{
    AttestationState, GatewayAccess, GetGatewayAccessRequest, GetHostStatusRequest, HostStatus,
    IdentityStatus, RuntimeState, ServiceStatus, SetGatewayStateRequest, SetProviderStateRequest,
};
use hellas_rpc::services::host_control::{HostControlHandler, HostControlServer};
use hellas_wire::mux::{MuxTransport, Role};
use hellas_wire::{Dispatcher, StreamTransport, TransportContext, WireCode, WireStatus};
use tokio::net::{UnixListener, UnixStream};

use crate::dto;
use crate::state::AppState;

pub async fn serve(state: Arc<AppState>) -> anyhow::Result<()> {
    prepare_socket(state.socket_path())?;
    let listener = UnixListener::bind(state.socket_path())?;
    std::fs::set_permissions(state.socket_path(), std::fs::Permissions::from_mode(0o600))?;

    loop {
        let (stream, _) = listener.accept().await?;
        if peer_uid(&stream)? != unsafe { libc::geteuid() } {
            tracing::warn!("rejected local-control connection from another uid");
            continue;
        }
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(error) = serve_connection(stream, state).await {
                tracing::warn!(%error, "local-control connection ended");
            }
        });
    }
}

async fn serve_connection(stream: UnixStream, state: Arc<AppState>) -> anyhow::Result<()> {
    let transport =
        hellas_sdk::local::transport(stream, Role::Server, TransportContext::default())?;
    let server = HostControlServer(Handler(state));
    while let Some(inbound) = transport.accept().await? {
        Dispatcher::<MuxTransport>::dispatch(&server, inbound).await?;
    }
    Ok(())
}

#[derive(Clone)]
struct Handler(Arc<AppState>);

#[allow(refining_impl_trait)]
impl HostControlHandler for Handler {
    async fn get_host_status(
        &self,
        _request: GetHostStatusRequest,
    ) -> Result<HostStatus, WireStatus> {
        Ok(to_proto(self.0.status().await))
    }

    async fn set_provider_state(
        &self,
        request: SetProviderStateRequest,
    ) -> Result<HostStatus, WireStatus> {
        self.0
            .set_provider_enabled(request.enabled, None)
            .await
            .map(to_proto)
            .map_err(|error| WireStatus::internal(error.to_string()))
    }

    async fn set_gateway_state(
        &self,
        request: SetGatewayStateRequest,
    ) -> Result<HostStatus, WireStatus> {
        self.0
            .set_gateway_enabled(request.enabled, None)
            .await
            .map(to_proto)
            .map_err(|error| WireStatus::internal(error.to_string()))
    }

    async fn get_gateway_access(
        &self,
        _request: GetGatewayAccessRequest,
    ) -> Result<GatewayAccess, WireStatus> {
        self.0
            .gateway_access()
            .await
            .map(|access| GatewayAccess {
                address: access.address,
                bearer: access.bearer,
            })
            .ok_or_else(|| WireStatus::new(WireCode::FailedPrecondition, "gateway is not running"))
    }
}

fn to_proto(status: dto::AppStatus) -> HostStatus {
    HostStatus {
        version: status.version,
        identity: Some(IdentityStatus {
            producer_id: status.identity.producer_id,
            caller_public_key: status.identity.caller_public_key,
            node_id: status.identity.node_id,
            attestation: match status.identity.attestation.as_str() {
                "apple-app-attest" => AttestationState::AppleAppAttest as i32,
                "software" => AttestationState::Software as i32,
                "available" => AttestationState::Enrolling as i32,
                _ => AttestationState::Unavailable as i32,
            },
            detail: status.identity.detail,
        }),
        provider: Some(service_to_proto(status.provider)),
        gateway: Some(service_to_proto(status.gateway)),
        endpoint: status.endpoint,
    }
}

fn service_to_proto(status: dto::ServiceStatus) -> ServiceStatus {
    ServiceStatus {
        state: match status.state {
            dto::ServiceState::Stopped => RuntimeState::Stopped as i32,
            dto::ServiceState::Starting => RuntimeState::Starting as i32,
            dto::ServiceState::Running => RuntimeState::Running as i32,
            dto::ServiceState::Stopping => RuntimeState::Stopping as i32,
            dto::ServiceState::Failed => RuntimeState::Failed as i32,
        },
        detail: status.detail,
    }
}

fn prepare_socket(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => {
            match std::os::unix::net::UnixStream::connect(path) {
                Ok(_) => Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another Gate process owns the local-control socket",
                )),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                    ) =>
                {
                    std::fs::remove_file(path)
                }
                Err(error) => Err(error),
            }
        }
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "refusing to replace a non-socket local-control path",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> io::Result<libc::uid_t> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &raw mut length,
        )
    };
    if result == 0 {
        Ok(credentials.uid)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))]
fn peer_uid(stream: &UnixStream) -> io::Result<libc::uid_t> {
    let mut uid = 0;
    let mut gid = 0;
    let result = unsafe { libc::getpeereid(stream.as_raw_fd(), &raw mut uid, &raw mut gid) };
    if result == 0 {
        Ok(uid)
    } else {
        Err(io::Error::last_os_error())
    }
}
