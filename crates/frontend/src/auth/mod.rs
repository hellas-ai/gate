//! Authentication module

pub mod auth_component;
pub mod component;
pub mod context;
pub mod error_messages;

// Re-export commonly used items
pub use auth_component::AuthComponent;
pub use component::Auth;
pub use context::{AuthAction, AuthProvider, use_auth, use_auth_state, use_is_authenticated};
