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
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserListResponse {
    pub users: Vec<UserInfo>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}

#[function_component(UserManagement)]
pub fn user_management() -> Html {
    let auth = use_auth();
    let users = use_state(Vec::<UserInfo>::new);
    let loading = use_state(|| true);
    let error = use_state(|| Option::<String>::None);

    // Load users on mount and ensure auth client is updated
    {
        let users = users.clone();
        let loading = loading.clone();
        let error = error.clone();
        let auth = auth.clone();

        use_effect_with(auth.auth_state.clone(), move |auth_state| {
            if let Some(auth_state) = auth_state {
                let token = auth_state.token.clone();
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
                    {"Manage user accounts"}
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
                                            <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500 dark:text-gray-400">
                                                {format_date(&user.created_at)}
                                            </td>
                                            <td class="px-6 py-4 whitespace-nowrap text-right text-sm font-medium">
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

fn format_date(date: &chrono::DateTime<chrono::Utc>) -> String {
    date.format("%Y-%m-%d").to_string()
}
