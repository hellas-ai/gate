pub mod api_wrapper;
pub mod auth;
pub mod bootstrap;
pub mod inference;
pub mod webauthn_browser;

pub use api_wrapper::{handle_api_error, with_auth_error_handling};
pub use auth::AuthApiService;
pub use bootstrap::{BootstrapService, BootstrapStatus};
pub use inference::{ChatMessage, InferenceService, Model, Role};
pub use webauthn_browser::WebAuthnBrowserService;
