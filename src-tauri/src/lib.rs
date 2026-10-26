#![recursion_limit = "256"]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]
pub mod apple;
pub mod apply;
pub mod chain;
pub mod clients;
#[cfg(feature = "desktop")]
pub mod commands;
pub mod contacts;
pub mod control;
pub mod credentials;
pub mod dto;
pub mod error;
pub mod gateway;
pub mod history;
pub mod identity;
#[cfg(test)]
mod integration_tests;
pub mod paid;
pub mod period;
pub mod provider_identity;
pub mod provider_setup;
pub mod run;
pub mod state;
pub mod trust;
pub mod vault;
