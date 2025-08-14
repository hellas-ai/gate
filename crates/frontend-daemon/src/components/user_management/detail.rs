//! User detail view with permission management

use super::shared::{ActionButton, ActionButtonVariant, EmptyState, StatusBadge};
use crate::services::user::{UserInfo, UserPermission, UserService};
use gloo::timers::callback::Timeout;
use yew::functional::use_memo;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct UserDetailProps {
    pub user_id: String,
    pub on_back: Callback<()>,
    pub can_manage_permissions: bool,
}

#[function_component(UserDetail)]
pub fn user_detail(props: &UserDetailProps) -> Html {
    let user_service = use_memo((), |_| UserService::new());

    let user = use_state(|| Option::<UserInfo>::None);
    let permissions = use_state(Vec::<UserPermission>::new);
    let is_loading = use_state(|| true);
    let error = use_state(|| Option::<String>::None);
    let success = use_state(|| Option::<String>::None);
    let show_add_permission = use_state(|| false);

    // Load user data
    {
        let user_id = props.user_id.clone();
        let user_service = user_service.clone();
        let user = user.clone();
        let permissions = permissions.clone();
        let is_loading = is_loading.clone();
        let error = error.clone();

        use_effect_with(user_id.clone(), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                is_loading.set(true);

                // Load user info
                match user_service.get_user(&user_id).await {
                    Ok(user_info) => {
                        user.set(Some(user_info));
                    }
                    Err(e) => {
                        error.set(Some(format!("Failed to load user: {e}")));
                    }
                }

                // Load permissions
                match user_service.get_user_permissions(&user_id).await {
                    Ok(perms) => {
                        permissions.set(perms);
                    }
                    Err(e) => {
                        error.set(Some(format!("Failed to load permissions: {e}")));
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

    let on_toggle_status = {
        let user = user.clone();
        let user_service = user_service.clone();
        let error = error.clone();
        let success = success.clone();
        let user_id = props.user_id.clone();

        Callback::from(move |_| {
            if let Some(user_info) = (*user).as_ref() {
                let enabled = !user_info.enabled;
                let user_service = user_service.clone();
                let user = user.clone();
                let error = error.clone();
                let success = success.clone();
                let user_id = user_id.clone();

                wasm_bindgen_futures::spawn_local(async move {
                    match user_service.update_user_status(&user_id, enabled).await {
                        Ok(updated_user) => {
                            user.set(Some(updated_user));
                            success.set(Some(format!(
                                "User {}",
                                if enabled { "enabled" } else { "disabled" }
                            )));
                        }
                        Err(e) => {
                            error.set(Some(format!("Failed to update status: {e}")));
                        }
                    }
                });
            }
        })
    };

    let on_revoke_permission = {
        let user_id = props.user_id.clone();
        let user_service = user_service.clone();
        let permissions = permissions.clone();
        let error = error.clone();
        let success = success.clone();

        Callback::from(move |perm: UserPermission| {
            let user_service = user_service.clone();
            let permissions = permissions.clone();
            let error = error.clone();
            let success = success.clone();
            let user_id = user_id.clone();

            wasm_bindgen_futures::spawn_local(async move {
                match user_service
                    .revoke_permission(&user_id, &perm.action, &perm.object)
                    .await
                {
                    Ok(_) => {
                        // Reload permissions
                        match user_service.get_user_permissions(&user_id).await {
                            Ok(perms) => {
                                permissions.set(perms);
                                success.set(Some("Permission revoked".to_string()));
                            }
                            Err(e) => {
                                error.set(Some(format!("Failed to reload permissions: {e}")));
                            }
                        }
                    }
                    Err(e) => {
                        error.set(Some(format!("Failed to revoke permission: {e}")));
                    }
                }
            });
        })
    };

    if *is_loading {
        return html! {
            <div class="flex justify-center items-center h-64">
                <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500"></div>
            </div>
        };
    }

    let Some(user_info) = (*user).as_ref() else {
        return html! {
            <div class="text-center py-12">
                <p class="text-red-600 dark:text-red-400">{"User not found"}</p>
                <button
                    onclick={props.on_back.reform(|_| ())}
                    class="mt-4 text-blue-600 hover:text-blue-800"
                >
                    {"← Back to list"}
                </button>
            </div>
        };
    };

    html! {
        <div class="space-y-6">
            // Back button
            <button
                onclick={props.on_back.reform(|_| ())}
                class="text-blue-600 hover:text-blue-800 dark:text-blue-400 dark:hover:text-blue-300 flex items-center"
            >
                <svg class="w-4 h-4 mr-1" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7" />
                </svg>
                {"Back to users"}
            </button>

            // Messages
            {if let Some(err) = (*error).as_ref() {
                html! {
                    <div class="p-4 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-md">
                        <p class="text-red-700 dark:text-red-300">{err}</p>
                    </div>
                }
            } else {
                html! {}
            }}

            {if let Some(msg) = (*success).as_ref() {
                html! {
                    <div class="p-4 bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800 rounded-md">
                        <p class="text-green-700 dark:text-green-300">{msg}</p>
                    </div>
                }
            } else {
                html! {}
            }}

            // User info card
            <div class="bg-white dark:bg-gray-800 rounded-lg shadow">
                <div class="px-6 py-4 border-b border-gray-200 dark:border-gray-700">
                    <div class="flex items-center justify-between">
                        <div class="flex items-center space-x-4">
                            <div class="w-16 h-16 bg-gradient-to-br from-blue-500 to-purple-600 rounded-full flex items-center justify-center text-white font-bold text-2xl">
                                {user_info.name.as_ref()
                                    .and_then(|n| n.chars().next())
                                    .unwrap_or_else(|| user_info.id.chars().next().unwrap_or('?'))
                                    .to_uppercase().to_string()}
                            </div>
                            <div>
                                <h2 class="text-xl font-semibold text-gray-900 dark:text-gray-100">
                                    {user_info.name.as_deref().unwrap_or(&user_info.id)}
                                </h2>
                                <p class="text-sm text-gray-500 dark:text-gray-400">
                                    {&user_info.id}
                                </p>
                            </div>
                        </div>
                        <StatusBadge enabled={user_info.enabled} />
                    </div>
                </div>

                <div class="px-6 py-4 space-y-4">
                    <div class="grid grid-cols-2 gap-4 text-sm">
                        <div>
                            <p class="text-gray-500 dark:text-gray-400">{"Created"}</p>
                            <p class="text-gray-900 dark:text-gray-100">
                                {user_info.created_at.format("%Y-%m-%d %H:%M").to_string()}
                            </p>
                        </div>
                        <div>
                            <p class="text-gray-500 dark:text-gray-400">{"Last Updated"}</p>
                            <p class="text-gray-900 dark:text-gray-100">
                                {user_info.updated_at.format("%Y-%m-%d %H:%M").to_string()}
                            </p>
                        </div>
                        {if let Some(disabled_at) = user_info.disabled_at {
                            html! {
                                <div>
                                    <p class="text-gray-500 dark:text-gray-400">{"Disabled At"}</p>
                                    <p class="text-gray-900 dark:text-gray-100">
                                        {disabled_at.format("%Y-%m-%d %H:%M").to_string()}
                                    </p>
                                </div>
                            }
                        } else {
                            html! {}
                        }}
                    </div>

                    <div class="flex space-x-3">
                        <ActionButton
                            label={if user_info.enabled { "Disable User" } else { "Enable User" }}
                            icon={html! {
                                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
                                        d={if user_info.enabled {
                                            "M18.364 18.364A9 9 0 005.636 5.636m12.728 12.728A9 9 0 015.636 5.636m12.728 12.728L5.636 5.636"
                                        } else {
                                            "M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
                                        }} />
                                </svg>
                            }}
                            on_click={on_toggle_status}
                            enabled={true}
                            variant={if user_info.enabled { ActionButtonVariant::Danger } else { ActionButtonVariant::Primary }}
                        />
                    </div>
                </div>
            </div>

            // Permissions section
            <div class="bg-white dark:bg-gray-800 rounded-lg shadow">
                <div class="px-6 py-4 border-b border-gray-200 dark:border-gray-700">
                    <div class="flex items-center justify-between">
                        <h3 class="text-lg font-medium text-gray-900 dark:text-gray-100">
                            {"Permissions"}
                        </h3>
                        {if props.can_manage_permissions {
                            let show_add = show_add_permission.clone();
                            html! {
                                <button
                                    onclick={Callback::from(move |_| show_add.set(true))}
                                    class="px-3 py-1 bg-blue-600 text-white text-sm rounded hover:bg-blue-700"
                                >
                                    {"+ Grant Permission"}
                                </button>
                            }
                        } else {
                            html! {}
                        }}
                    </div>
                </div>

                <div class="px-6 py-4">
                    {if permissions.is_empty() {
                        html! {
                            <EmptyState
                                title="No permissions"
                                description="This user has no explicit permissions granted"
                                icon={html! {
                                    <svg fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
                                            d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
                                    </svg>
                                }}
                            />
                        }
                    } else {
                        html! {
                            <div class="space-y-2">
                                {permissions.iter().map(|perm| {
                                    let perm_clone = perm.clone();
                                    html! {
                                        <div key={format!("{}-{}", perm.action, perm.object)}
                                             class="flex items-center justify-between p-3 bg-gray-50 dark:bg-gray-900 rounded-lg">
                                            <div>
                                                <p class="font-medium text-gray-900 dark:text-gray-100">
                                                    {&perm.action}
                                                </p>
                                                <p class="text-sm text-gray-500 dark:text-gray-400">
                                                    {&perm.object}
                                                </p>
                                                <p class="text-xs text-gray-400 dark:text-gray-500 mt-1">
                                                    {format!("Granted: {}", perm.granted_at.format("%Y-%m-%d %H:%M"))}
                                                </p>
                                            </div>
                                            {if props.can_manage_permissions {
                                                html! {
                                                    <button
                                                        onclick={on_revoke_permission.reform(move |_| perm_clone.clone())}
                                                        class="text-red-600 hover:text-red-800 dark:text-red-400 dark:hover:text-red-300"
                                                    >
                                                        {"Revoke"}
                                                    </button>
                                                }
                                            } else {
                                                html! {}
                                            }}
                                        </div>
                                    }
                                }).collect::<Html>()}
                            </div>
                        }
                    }}
                </div>
            </div>

            // Add Permission Modal
            {if *show_add_permission {
                let show_add_permission_close = show_add_permission.clone();
                let show_add_permission_grant = show_add_permission.clone();
                let permissions = permissions.clone();
                let user_service = user_service.clone();
                let user_id = props.user_id.clone();

                html! {
                    <PermissionGrantModal
                        user_id={props.user_id.clone()}
                        on_close={Callback::from(move |_| show_add_permission_close.set(false))}
                        on_grant={Callback::from(move |_| {
                            show_add_permission_grant.set(false);
                            // Reload permissions
                            let user_service = user_service.clone();
                            let permissions = permissions.clone();
                            let user_id = user_id.clone();
                            wasm_bindgen_futures::spawn_local(async move {
                                if let Ok(perms) = user_service.get_user_permissions(&user_id).await {
                                    permissions.set(perms);
                                }
                            });
                        })}
                    />
                }
            } else {
                html! {}
            }}
        </div>
    }
}

#[derive(Properties, PartialEq)]
struct PermissionGrantModalProps {
    user_id: String,
    on_close: Callback<()>,
    on_grant: Callback<()>,
}

#[function_component(PermissionGrantModal)]
fn permission_grant_modal(props: &PermissionGrantModalProps) -> Html {
    let action = use_state(|| "Read".to_string());
    let object = use_state(|| "local/System/*".to_string());

    let available_actions = [
        "Read",
        "Write",
        "Delete",
        "Execute",
        "Manage",
        "GrantPermission",
        "RevokePermission",
        "ViewPermissions",
    ];

    let on_submit = {
        let user_service = UserService::new();
        let user_id = props.user_id.clone();
        let action = action.clone();
        let object = object.clone();
        let on_grant = props.on_grant.clone();

        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();

            let user_service = user_service.clone();
            let user_id = user_id.clone();
            let action_val = (*action).clone();
            let object_val = (*object).clone();
            let on_grant = on_grant.clone();

            wasm_bindgen_futures::spawn_local(async move {
                match user_service
                    .grant_permission(&user_id, &action_val, &object_val)
                    .await
                {
                    Ok(_) => {
                        on_grant.emit(());
                    }
                    Err(e) => {
                        web_sys::window().and_then(|w| {
                            w.alert_with_message(&format!("Failed to grant permission: {e}"))
                                .ok()
                        });
                    }
                }
            });
        })
    };

    html! {
        <div class="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
            <div class="bg-white dark:bg-gray-800 rounded-lg p-6 max-w-md w-full mx-4">
                <h3 class="text-lg font-semibold mb-4">{"Grant Permission"}</h3>

                <form onsubmit={on_submit}>
                    <div class="space-y-4">
                        <div>
                            <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                                {"Action"}
                            </label>
                            <select
                                class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md
                                       bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100"
                                value={(*action).clone()}
                                onchange={{
                                    let action = action.clone();
                                    Callback::from(move |e: Event| {
                                        let select: web_sys::HtmlSelectElement = e.target_unchecked_into();
                                        action.set(select.value());
                                    })
                                }}
                            >
                                {available_actions.iter().map(|a| html! {
                                    <option value={a.to_string()}>{a.to_string()}</option>
                                }).collect::<Html>()}
                            </select>
                        </div>

                        <div>
                            <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                                {"Object (namespace/kind/id)"}
                            </label>
                            <input
                                type="text"
                                class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-md
                                       bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100"
                                value={(*object).clone()}
                                oninput={{
                                    let object = object.clone();
                                    Callback::from(move |e: InputEvent| {
                                        let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                        object.set(input.value());
                                    })
                                }}
                                placeholder="e.g., local/System/*, system/User/user123"
                            />
                        </div>
                    </div>

                    <div class="mt-6 flex justify-end space-x-3">
                        <button
                            type="button"
                            onclick={props.on_close.reform(|_| ())}
                            class="px-4 py-2 text-gray-700 dark:text-gray-300 bg-gray-100 dark:bg-gray-700
                                   hover:bg-gray-200 dark:hover:bg-gray-600 rounded-md"
                        >
                            {"Cancel"}
                        </button>
                        <button
                            type="submit"
                            class="px-4 py-2 bg-blue-600 text-white hover:bg-blue-700 rounded-md"
                        >
                            {"Grant"}
                        </button>
                    </div>
                </form>
            </div>
        </div>
    }
}
