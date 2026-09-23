#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
#[cfg(unix)]
use std::path::Path;
use std::sync::Arc;

use hellas_rpc::pb::host::{
    AttestationState, GatewayAccess, GetGatewayAccessRequest, GetHostStatusRequest, HostStatus,
    IdentityStatus, RuntimeState, ServiceStatus, SetGatewayStateRequest, SetProviderStateRequest,
};
use hellas_rpc::services::host_control::{HostControlHandler, HostControlServer};
#[cfg(unix)]
use hellas_wire::mux::{MuxTransport, Role};
#[cfg(unix)]
use hellas_wire::{Dispatcher, StreamTransport, TransportContext};
use hellas_wire::{WireCode, WireStatus};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

use crate::dto;
use crate::state::AppState;

#[cfg(unix)]
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

/// Windows: hellas's owner-authenticated named pipe, derived from the same
/// `gate.sock` path so hellas-cli finds it. It does what the socket
/// permissions and peer-uid check do on Unix: an owner-only DACL, a first
/// instance nobody could have squatted, remote clients refused, and clients
/// admitted only if their process runs as this user.
#[cfg(windows)]
pub async fn serve(state: Arc<AppState>) -> anyhow::Result<()> {
    let _server = hellas_sdk::local::LocalControlServer::bind(
        state.socket_path(),
        HostControlServer(Handler(state.clone())),
    )?;
    std::future::pending::<()>().await;
    Ok(())
}

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use hellas_rpc::services::host_control::HostControlClientImpl;

    /// Gate's listener answers over the platform's local transport: a Unix
    /// socket, or on Windows hellas's owner-authenticated named pipe.
    #[tokio::test]
    async fn host_status_is_served_over_the_local_transport() {
        let directory = tempfile::tempdir().unwrap();
        hellas_private::restrict_directory(directory.path()).unwrap();
        let state = Arc::new(AppState::open(directory.path()).unwrap());
        let socket = state.socket_path().to_path_buf();
        tokio::spawn(serve(state.clone()));

        let mut connection = None;
        for _ in 0..100 {
            match hellas_sdk::local::connect(&socket).await {
                Ok(transport) => {
                    connection = Some(transport);
                    break;
                }
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
            }
        }
        let host = HostControlClientImpl::new(connection.expect("listener came up"));
        let status = host.get_host_status(GetHostStatusRequest {}).await.unwrap();
        assert_eq!(status.version, state.status().await.version);
    }
}
