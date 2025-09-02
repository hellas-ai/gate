use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("User not found: {0}")]
    UserNotFound(String),

    #[error("API key not found")]
    ApiKeyNotFound,

    #[error("Invalid API key")]
    InvalidApiKey,

    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Insufficient permissions")]
    Unauthorized,

    #[error("Request rejected: {0}")]
    Rejected(http::StatusCode, String),

    #[error("Redirect required: {0}")]
    Redirect(String),

    #[error("State backend error: {0}")]
    StateError(String),

    #[error("Plugin error: {0}")]
    PluginError(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Internal error: {0}")]
    Internal(String),

    // Router-specific errors
    #[error("No sinks available for routing")]
    NoSinksAvailable,

    #[error("Protocol conversion not supported: {0} to {1}")]
    UnsupportedConversion(String, String),

    #[error("Quota exceeded: {0}")]
    QuotaExceeded(String),

    #[error("All routes failed")]
    AllRoutesFailed,

    #[error("Model not supported by any sink: {0}")]
    ModelNotSupported(String),

    #[error("Invalid routing configuration: {0}")]
    InvalidRoutingConfig(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

pub type Result<T> = std::result::Result<T, Error>;

// Cloudflare-specific error conversions
#[cfg(feature = "cloudflare")]
mod cloudflare_impls {
    use super::Error;

    impl From<worker::Error> for Error {
        fn from(err: worker::Error) -> Self {
            Error::StateError(format!("Worker error: {err}"))
        }
    }

    impl From<worker::kv::KvError> for Error {
        fn from(err: worker::kv::KvError) -> Self {
            Error::StateError(format!("KV error: {err}"))
        }
    }

    impl From<worker::d1::D1Error> for Error {
        fn from(err: worker::d1::D1Error) -> Self {
            Error::StateError(format!("D1 error: {err}"))
        }
    }

    impl From<sqlx_d1::Error> for Error {
        fn from(err: sqlx_d1::Error) -> Self {
            Error::StateError(format!("D1 error: {err}"))
        }
    }
}
