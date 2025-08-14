//! User management container component

use super::detail::UserDetail;
use super::list::UserList;
use crate::services::user::{UserInfo, UserService};
use gate_frontend_common::auth::use_auth;
use gloo::timers::callback::Timeout;
use yew::prelude::*;

pub enum View {
    List,
    Detail(String),
}

#[function_component(UserManagementContainer)]
pub fn user_management_container() -> Html {
    let auth = use_auth();
    let user_service = use_memo((), |_| UserService::new());

    let users = use_state(Vec::<UserInfo>::new);
    let view = use_state(|| View::List);
    let is_loading = use_state(|| true);
    let error = use_state(|| Option::<String>::None);
    let success = use_state(|| Option::<String>::None);

    let current_user_id = auth
        .auth_state
        .as_ref()
        .map(|s| s.user_id.clone())
        .unwrap_or_default();

    // For now, treat all authenticated users as having manage permission
    // TODO: Check actual permissions from backend
    let can_manage_users = true;

    // Load users effect
    {
        let users = users.clone();
        let is_loading = is_loading.clone();
        let error = error.clone();
        let user_service = user_service.clone();

        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                is_loading.set(true);
                match user_service.list_users(1, 50, None).await {
                    Ok(response) => {
                        users.set(response.users);
                        error.set(None);
                    }
                    Err(e) => {
                        error.set(Some(format!("Failed to load users: {e}")));
                    }
                }
                is_loading.set(false);
            });
        });
    }

    // Clear success message after timeout
    {
        let success = success.clone();
        use_effect_with(success.clone(), move |msg| {
            if msg.is_some() {
                let success = success.clone();
                Timeout::new(3000, move || {
                    success.set(None);
                })
                .forget();
            }
        });
    }

    let reload_users = {
        let users = users.clone();
        let user_service = user_service.clone();
        let error = error.clone();

        Callback::from(move |_: ()| {
            let users = users.clone();
            let user_service = user_service.clone();
            let error = error.clone();

            wasm_bindgen_futures::spawn_local(async move {
                match user_service.list_users(1, 50, None).await {
                    Ok(response) => {
                        users.set(response.users);
                        error.set(None);
                    }
                    Err(e) => {
                        error.set(Some(format!("Failed to reload users: {e}")));
                    }
                }
            });
        })
    };

    let on_user_select = {
        let view = view.clone();
        Callback::from(move |user_id: String| {
            view.set(View::Detail(user_id));
        })
    };

    let on_user_delete = {
        let user_service = user_service.clone();
        let error = error.clone();
        let success = success.clone();
        let reload = reload_users.clone();

        Callback::from(move |user_id: String| {
            if web_sys::window()
                .and_then(|w| {
                    w.confirm_with_message(&format!("Delete user {user_id}?"))
                        .ok()
                })
                .unwrap_or(false)
            {
                let user_service = user_service.clone();
                let error = error.clone();
                let success = success.clone();
                let reload = reload.clone();

                wasm_bindgen_futures::spawn_local(async move {
                    match user_service.delete_user(&user_id).await {
                        Ok(_) => {
                            success.set(Some(format!("User {user_id} deleted")));
                            reload.emit(());
                        }
                        Err(e) => {
                            error.set(Some(format!("Failed to delete user: {e}")));
                        }
                    }
                });
            }
        })
    };

    let on_user_toggle = {
        let user_service = user_service.clone();
        let error = error.clone();
        let success = success.clone();
        let reload = reload_users.clone();

        Callback::from(move |(user_id, enabled): (String, bool)| {
            let user_service = user_service.clone();
            let error = error.clone();
            let success = success.clone();
            let reload = reload.clone();

            wasm_bindgen_futures::spawn_local(async move {
                match user_service.update_user_status(&user_id, enabled).await {
                    Ok(_) => {
                        success.set(Some(format!(
                            "User {} {}",
                            user_id,
                            if enabled { "enabled" } else { "disabled" }
                        )));
                        reload.emit(());
                    }
                    Err(e) => {
                        error.set(Some(format!("Failed to update user status: {e}")));
                    }
                }
            });
        })
    };

    let on_search = Callback::from(move |_search: String| {
        // TODO: Implement search
    });

    html! {
        <div class="p-6 max-w-7xl mx-auto">
            <div class="mb-6">
                <h1 class="text-2xl font-bold text-gray-900 dark:text-gray-100">
                    {"User Management"}
                </h1>
                <p class="mt-1 text-sm text-gray-600 dark:text-gray-400">
                    {"Manage user accounts and permissions"}
                </p>
            </div>

            {if let Some(err) = (*error).as_ref() {
                html! {
                    <div class="mb-4 p-4 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-md">
                        <p class="text-red-700 dark:text-red-300">{err}</p>
                    </div>
                }
            } else {
                html! {}
            }}

            {if let Some(msg) = (*success).as_ref() {
                html! {
                    <div class="mb-4 p-4 bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800 rounded-md">
                        <p class="text-green-700 dark:text-green-300">{msg}</p>
                    </div>
                }
            } else {
                html! {}
            }}

            {match &*view {
                View::List => html! {
                    <UserList
                        users={(*users).clone()}
                        is_loading={*is_loading}
                        current_user_id={current_user_id}
                        on_user_select={on_user_select}
                        on_user_delete={on_user_delete}
                        on_user_toggle={on_user_toggle}
                        on_search={on_search}
                        can_manage_users={can_manage_users}
                    />
                },
                View::Detail(user_id) => {
                    let view_clone = view.clone();
                    html! {
                        <UserDetail
                            user_id={user_id.clone()}
                            on_back={Callback::from(move |_| view_clone.set(View::List))}
                            can_manage_permissions={can_manage_users}
                        />
                    }
                }
            }}
        </div>
    }
}
