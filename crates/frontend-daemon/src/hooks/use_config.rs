use crate::services::config::ConfigApiService;
use gate_frontend_common::auth::context::use_auth_client;
use yew::prelude::*;

#[hook]
pub fn use_config() -> Option<ConfigApiService> {
    let client = use_auth_client()?;
    Some(ConfigApiService::new(client))
}
