#![feature(let_chains)]

pub mod auth;
pub mod client;
pub mod components;
pub mod config;
pub mod hooks;
pub mod services;
pub mod theme;

pub use auth::context::AuthContext;
pub use client::WrappedAuthClient;
pub use components::{LiveChat, Spinner, ThemeToggle};
pub use config::AuthConfig;
pub use gate_http::client::error::ClientError;
pub use theme::{Theme, ThemeContext, ThemeProvider};
