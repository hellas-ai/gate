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
mod mutations;
pub mod paid;
pub mod period;
pub mod provider_identity;
pub mod provider_setup;
#[cfg(test)]
mod responses_fixture;
pub mod run;
pub mod settings;
pub mod state;
pub mod trust;
mod usage;
pub mod vault;
