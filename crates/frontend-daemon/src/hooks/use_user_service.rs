use crate::services::user::UserService;
use gate_frontend_common::auth::context::use_auth_client;
use yew::prelude::*;

#[hook]
pub fn use_user_service() -> Option<UserService> {
    let client = use_auth_client()?;
    Some(UserService::new(client))
}
