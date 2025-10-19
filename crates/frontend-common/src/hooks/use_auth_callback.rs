//! Authentication callback hooks

use crate::auth::use_auth;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

/// Hook to create authenticated request callbacks
/// Use this in components to create callbacks that need auth
#[hook]
pub fn use_auth_callback<F, Fut, T>(f: F) -> Callback<T>
where
    F: Fn(T, Option<String>) -> Fut + 'static,
    Fut: std::future::Future<Output = ()> + 'static,
    T: 'static,
{
    let auth = use_auth();
    let token = match &auth.state {
        crate::auth::context::AuthState::Authenticated { token, .. } => Some(token.clone()),
        _ => None,
    };

    Callback::from(move |event: T| {
        let token = token.clone();
        let future = f(event, token);
        spawn_local(future);
    })
}

/// Hook to create a simple callback for API requests
#[hook]
pub fn use_api_callback<F, Fut>(f: F) -> Callback<MouseEvent>
where
    F: Fn(Option<String>) -> Fut + 'static,
    Fut: std::future::Future<Output = ()> + 'static,
{
    use_auth_callback(move |_: MouseEvent, token| f(token))
}
