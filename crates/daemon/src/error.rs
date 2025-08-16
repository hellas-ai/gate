use gate_core::access::PermissionDenied;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("Permission denied: {0}")]
    PermissionDenied(#[from] PermissionDenied),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(String),

    #[error("TLS forward error: {0}")]
    TlsForward(String),

    #[error("Channel send error")]
    ChannelSend,

    #[error("Channel receive error")]
    ChannelRecv(#[from] oneshot::error::RecvError),

    #[error("Platform directories could not be determined")]
    PlatformDirsNotFound,
}

impl<T> From<mpsc::error::SendError<T>> for DaemonError {
    fn from(_: mpsc::error::SendError<T>) -> Self {
        DaemonError::ChannelSend
    }
}

pub type Result<T> = std::result::Result<T, DaemonError>;
