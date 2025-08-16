//! Service layer for business logic

pub mod identity;

#[cfg(not(target_arch = "wasm32"))]
pub mod jwt;

pub use identity::{HttpContext, HttpIdentity};

#[cfg(not(target_arch = "wasm32"))]
pub use jwt::{Claims, JwtConfig, JwtService};
