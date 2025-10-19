//! Custom hooks for the application

pub mod use_auth_callback;
pub mod use_inference;
pub mod use_webauthn;

pub use use_inference::use_inference;
pub use use_webauthn::{use_webauthn, WebAuthnState};
