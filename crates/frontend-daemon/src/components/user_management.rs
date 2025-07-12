//! User management component for admins

use gate_frontend_common::{
    auth::use_auth,
    client::{create_authenticated_client, set_auth_token},
    components::Spinner as LoadingSpinner,
};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use yew::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserInfo {
    pub id: String,
    pub name: Option<String>,
    pub role: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserListResponse {
    pub users: Vec<UserInfo>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserRoleRequest {
    pub role: String,
}

#[function_component(UserManagement)]
pub fn user_management() -> Html {
    let auth = use_auth();
    let users = use_state(Vec::<UserInfo>::new);
    let loading = use_state(|| true);
    let error = use_state(|| Option::<String>::None);
    let selected_user = use_state(|| Option::<UserInfo>::None);
    let show_role_dialog = use_state(|| false);
    let new_role = use_state(|| "user".to_string());

    // Load users on mount and ensure auth client is updated
    {
        let users = users.clone();
        let loading = loading.clone();
        let error = error.clone();
        let auth = auth.clone();

        use_effect_with(auth.auth_state.clone(), move |auth_state| {
            if let Some(auth_state) = auth_state {
                let token = auth_state.token.clone();
                // Ensure the typed auth client is updated
                let _ = set_auth_token(Some(&token));

                wasm_bindgen_futures::spawn_local(async move {
                    loading.set(true);
                    match load_users().await {
                        Ok(user_list) => {
                            users.set(user_list);
                            error.set(None);
                        }
                        Err(e) => {
                            error.set(Some(e));
                        }
                    }
                    loading.set(false);
                });
            }
            || ()
        });
    }

    // Handle role change
    let on_change_role = {
        let selected_user = selected_user.clone();
        let show_role_dialog = show_role_dialog.clone();
        let new_role = new_role.clone();
        Callback::from(move |user: UserInfo| {
            selected_user.set(Some(user.clone()));
            new_role.set(user.role);
            show_role_dialog.set(true);
        })
    };

    // Handle role update
    let on_update_role = {
        let selected_user = selected_user.clone();
        let show_role_dialog = show_role_dialog.clone();
        let new_role = new_role.clone();
        let users = users.clone();
        let error = error.clone();
        let auth = auth.clone();

        Callback::from(move |_| {
            if let Some(user) = (*selected_user).as_ref() {
                if let Some(auth_state) = &auth.auth_state {
                    let token = auth_state.token.clone();
                    let user_id = user.id.clone();
                    let role = (*new_role).clone();
                    let users = users.clone();
                    let error = error.clone();
                    let show_role_dialog = show_role_dialog.clone();

                    wasm_bindgen_futures::spawn_local(async move {
                        match update_user_role(&token, &user_id, &role).await {
                            Ok(_) => {
                                // Reload users
                                if let Ok(user_list) = load_users().await {
                                    users.set(user_list);
                                }
                                show_role_dialog.set(false);
                            }
                            Err(e) => {
                                error.set(Some(e));
                            }
                        }
                    });
                }
            }
        })
    };

    // Handle user deletion
    let on_delete_user = {
        let users = users.clone();
        let error = error.clone();
        let auth = auth.clone();

        Callback::from(move |user: UserInfo| {
            if let Some(auth_state) = &auth.auth_state {
                let token = auth_state.token.clone();
                let user_id = user.id.clone();
                let users = users.clone();
                let error = error.clone();

                if web_sys::window()
                    .and_then(|w| {
                        w.confirm_with_message(&format!(
                            "Are you sure you want to delete user '{}'?",
                            user.name.as_deref().unwrap_or(&user.id)
                        ))
                        .ok()
                    })
                    .unwrap_or(false)
                {
                    wasm_bindgen_futures::spawn_local(async move {
                        match delete_user(&token, &user_id).await {
                            Ok(_) => {
                                // Reload users
                                if let Ok(user_list) = load_users().await {
                                    users.set(user_list);
                                }
                            }
                            Err(e) => {
                                error.set(Some(e));
                            }
                        }
                    });
                }
            }
        })
    };

    html! {
        <div class="bg-white dark:bg-gray-900 rounded-lg shadow-sm">
            <div class="p-6 border-b border-gray-200 dark:border-gray-700">
                <h2 class="text-xl font-semibold text-gray-900 dark:text-white">
                    {"User Management"}
                </h2>
                <p class="mt-1 text-sm text-gray-600 dark:text-gray-400">
                    {"Manage user accounts and roles"}
                </p>
            </div>

            {if *loading {
                html! {
                    <div class="p-8 text-center">
                        <LoadingSpinner text={Some("Loading users...".to_string())} />
                    </div>
                }
            } else {
                html! {
                    <div class="overflow-x-auto">
                        <table class="w-full">
                            <thead class="bg-gray-50 dark:bg-gray-800">
                                <tr>
                                    <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                                        {"User"}
                                    </th>
                                    <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                                        {"Role"}
                                    </th>
                                    <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                                        {"Created"}
                                    </th>
                                    <th class="px-6 py-3 text-right text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                                        {"Actions"}
                                    </th>
                                </tr>
                            </thead>
                            <tbody class="bg-white dark:bg-gray-900 divide-y divide-gray-200 dark:divide-gray-700">
                                {users.iter().map(|user| {
                                    let user_clone = user.clone();
                                    let on_change_role = on_change_role.clone();
                                    let on_delete_user = on_delete_user.clone();
                                    let current_user_id = auth.auth_state.as_ref().map(|s| &s.user_id);
                                    let is_current_user = Some(&user.id) == current_user_id;

                                    html! {
                                        <tr key={user.id.clone()}>
                                            <td class="px-6 py-4 whitespace-nowrap">
                                                <div>
                                                    <div class="text-sm font-medium text-gray-900 dark:text-white">
                                                        {user.name.as_deref().unwrap_or("Unnamed")}
                                                    </div>
                                                    <div class="text-sm text-gray-500 dark:text-gray-400">
                                                        {&user.id}
                                                    </div>
                                                </div>
                                            </td>
                                            <td class="px-6 py-4 whitespace-nowrap">
                                                <span class={format!(
                                                    "inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium {}",
                                                    if user.role == "admin" {
                                                        "bg-purple-100 text-purple-800 dark:bg-purple-900 dark:text-purple-200"
                                                    } else {
                                                        "bg-gray-100 text-gray-800 dark:bg-gray-700 dark:text-gray-200"
                                                    }
                                                )}>
                                                    {&user.role}
                                                </span>
                                            </td>
                                            <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500 dark:text-gray-400">
                                                {format_date(&user.created_at)}
                                            </td>
                                            <td class="px-6 py-4 whitespace-nowrap text-right text-sm font-medium">
                                                <button
                                                    onclick={
                                                        let user = user_clone.clone();
                                                        move |_| on_change_role.emit(user.clone())
                                                    }
                                                    class="text-blue-600 hover:text-blue-900 dark:text-blue-400 dark:hover:text-blue-300 mr-4"
                                                    disabled={is_current_user}
                                                    title={if is_current_user { "Cannot change your own role" } else { "Change role" }}
                                                >
                                                    {"Edit Role"}
                                                </button>
                                                <button
                                                    onclick={
                                                        let user = user_clone;
                                                        move |_| on_delete_user.emit(user.clone())
                                                    }
                                                    class="text-red-600 hover:text-red-900 dark:text-red-400 dark:hover:text-red-300 disabled:opacity-50 disabled:cursor-not-allowed"
                                                    disabled={is_current_user}
                                                    title={if is_current_user { "Cannot delete your own account" } else { "Delete user" }}
                                                >
                                                    {"Delete"}
                                                </button>
                                            </td>
                                        </tr>
                                    }
                                }).collect::<Html>()}
                            </tbody>
                        </table>
                    </div>
                }
            }}

            {if let Some(error_msg) = (*error).as_ref() {
                html! {
                    <div class="p-4 bg-red-50 dark:bg-red-900/20 border-t border-red-200 dark:border-red-800">
                        <p class="text-sm text-red-800 dark:text-red-200">{error_msg}</p>
                    </div>
                }
            } else {
                html! {}
            }}

            // Role change dialog
            {if *show_role_dialog {
                html! {
                    <div class="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50">
                        <div class="bg-white dark:bg-gray-800 rounded-lg p-6 max-w-sm w-full mx-4">
                            <h3 class="text-lg font-semibold mb-4 text-gray-900 dark:text-white">
                                {"Change User Role"}
                            </h3>

                            {if let Some(user) = (*selected_user).as_ref() {
                                let display_name = user.name.as_deref().unwrap_or(&user.id);
                                html! {
                                    <>
                                        <p class="text-sm text-gray-600 dark:text-gray-400 mb-4">
                                            {format!("Changing role for: {display_name}")}
                                        </p>

                                        <div class="mb-6">
                                            <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
                                                {"New Role"}
                                            </label>
                                            <select
                                                class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-white"
                                                value={(*new_role).clone()}
                                                onchange={
                                                    let new_role = new_role.clone();
                                                    move |e: Event| {
                                                        let select: web_sys::HtmlSelectElement = e.target_unchecked_into();
                                                        new_role.set(select.value());
                                                    }
                                                }
                                            >
                                                <option value="user">{"User"}</option>
                                                <option value="admin">{"Admin"}</option>
                                            </select>
                                        </div>
                                    </>
                                }
                            } else {
                                html! {}
                            }}

                            <div class="flex gap-3">
                                <button
                                    onclick={on_update_role}
                                    class="flex-1 px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700"
                                >
                                    {"Update Role"}
                                </button>
                                <button
                                    onclick={
                                        let show_role_dialog = show_role_dialog.clone();
                                        move |_| show_role_dialog.set(false)
                                    }
                                    class="flex-1 px-4 py-2 bg-gray-200 dark:bg-gray-700 text-gray-800 dark:text-gray-200 rounded-lg hover:bg-gray-300 dark:hover:bg-gray-600"
                                >
                                    {"Cancel"}
                                </button>
                            </div>
                        </div>
                    </div>
                }
            } else {
                html! {}
            }}
        </div>
    }
}

// Helper functions
async fn load_users() -> Result<Vec<UserInfo>, String> {
    let client = create_authenticated_client()
        .map_err(|e| format!("Failed to create client: {e}"))?
        .ok_or_else(|| "Not authenticated".to_string())?;

    // The authenticated client already has the token, so we don't need to add headers
    let response = client
        .request(Method::GET, "/api/admin/users")
        .send()
        .await
        .map_err(|e| format!("Failed to load users: {e}"))?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("Failed to load users: {error_text}"));
    }

    let user_list: UserListResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse users: {e}"))?;

    Ok(user_list.users)
}

async fn update_user_role(_token: &str, user_id: &str, role: &str) -> Result<(), String> {
    let client = create_authenticated_client()
        .map_err(|e| format!("Failed to create client: {e}"))?
        .ok_or_else(|| "Not authenticated".to_string())?;

    let request = UpdateUserRoleRequest {
        role: role.to_string(),
    };

    let response = client
        .request(Method::PUT, &format!("/api/admin/users/{user_id}/role"))
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Failed to update role: {e}"))?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("Failed to update role: {error_text}"));
    }

    Ok(())
}

async fn delete_user(_token: &str, user_id: &str) -> Result<(), String> {
    let client = create_authenticated_client()
        .map_err(|e| format!("Failed to create client: {e}"))?
        .ok_or_else(|| "Not authenticated".to_string())?;

    let response = client
        .request(Method::DELETE, &format!("/api/admin/users/{user_id}"))
        .send()
        .await
        .map_err(|e| format!("Failed to delete user: {e}"))?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("Failed to delete user: {error_text}"));
    }

    Ok(())
}

fn format_date(date_str: &str) -> String {
    // Simple date formatting - in production you'd use a proper date parsing library
    date_str.split('T').next().unwrap_or(date_str).to_string()
}
