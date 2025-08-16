extern crate tracing;

mod common;
mod webauthn;

#[cfg(feature = "sqlite")]
mod sqlite;

#[cfg(feature = "sqlite")]
pub use webauthn::{SqlxWebAuthnBackend, StoredCredential};

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteStateBackend;

#[cfg(feature = "sqlite")]
pub type SqliteWebAuthnBackend = SqlxWebAuthnBackend<sqlx::Sqlite>;
