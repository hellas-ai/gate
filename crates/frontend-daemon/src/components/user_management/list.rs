//! User list view component

use super::shared::{EmptyState, StatusBadge, UserListSkeleton};
use crate::services::user::UserInfo;
use web_sys::HtmlInputElement;
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct UserListProps {
    pub users: Vec<UserInfo>,
    pub is_loading: bool,
    pub current_user_id: String,
    pub on_user_select: Callback<String>,
    pub on_user_delete: Callback<String>,
    pub on_user_toggle: Callback<(String, bool)>,
    pub on_search: Callback<String>,
    pub can_manage_users: bool,
}

#[function_component(UserList)]
pub fn user_list(props: &UserListProps) -> Html {
    let search_value = use_state(String::new);

    let on_search_input = {
        let search_value = search_value.clone();
        let on_search = props.on_search.clone();
        Callback::from(move |e: InputEvent| {
            let input: HtmlInputElement = e.target_unchecked_into();
            let value = input.value();
            search_value.set(value.clone());
            on_search.emit(value);
        })
    };

    if props.is_loading {
        return html! { <UserListSkeleton /> };
    }

    if props.users.is_empty() {
        return html! {
            <EmptyState
                title="No users found"
                description={if (*search_value).is_empty() {
                    "No users have been created yet.".to_string()
                } else {
                    format!("No users match '{}'", *search_value)
                }}
                icon={html! {
                    <svg fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
                            d="M12 4.354a4 4 0 110 5.292M15 21H3v-1a6 6 0 0112 0v1zm0 0h6v-1a6 6 0 00-9-5.197M13 7a4 4 0 11-8 0 4 4 0 018 0z" />
                    </svg>
                }}
            />
        };
    }

    html! {
        <div class="space-y-4">
            // Search bar
            <div class="relative">
                <div class="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                    <svg class="h-5 w-5 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
                            d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
                    </svg>
                </div>
                <input
                    type="text"
                    class="block w-full pl-10 pr-3 py-2 border border-gray-300 dark:border-gray-600
                           rounded-md leading-5 bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100
                           placeholder-gray-500 focus:outline-none focus:placeholder-gray-400 
                           focus:ring-1 focus:ring-blue-500 focus:border-blue-500 sm:text-sm"
                    placeholder="Search users..."
                    value={(*search_value).clone()}
                    oninput={on_search_input}
                />
            </div>

            // User list
            <div class="bg-white dark:bg-gray-800 shadow overflow-hidden rounded-lg">
                <table class="min-w-full divide-y divide-gray-200 dark:divide-gray-700">
                    <thead class="bg-gray-50 dark:bg-gray-900">
                        <tr>
                            <th scope="col" class="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                                {"User"}
                            </th>
                            <th scope="col" class="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                                {"Status"}
                            </th>
                            <th scope="col" class="px-6 py-3 text-left text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                                {"Created"}
                            </th>
                            <th scope="col" class="relative px-6 py-3">
                                <span class="sr-only">{"Actions"}</span>
                            </th>
                        </tr>
                    </thead>
                    <tbody class="bg-white dark:bg-gray-800 divide-y divide-gray-200 dark:divide-gray-700">
                        {props.users.iter().map(|user| {
                            let is_current_user = user.id == props.current_user_id;
                            let user_id = user.id.clone();

                            let on_view = {
                                let user_id = user_id.clone();
                                let on_select = props.on_user_select.clone();
                                Callback::from(move |_| on_select.emit(user_id.clone()))
                            };

                            let on_toggle = {
                                let user_id = user_id.clone();
                                let enabled = user.enabled;
                                let on_toggle = props.on_user_toggle.clone();
                                Callback::from(move |_| on_toggle.emit((user_id.clone(), !enabled)))
                            };

                            let on_delete = {
                                let user_id = user_id.clone();
                                let on_delete = props.on_user_delete.clone();
                                Callback::from(move |_| on_delete.emit(user_id.clone()))
                            };

                            html! {
                                <tr key={user.id.clone()}>
                                    <td class="px-6 py-4 whitespace-nowrap">
                                        <div class="flex items-center">
                                            <div class="flex-shrink-0 h-10 w-10">
                                                <div class="h-10 w-10 bg-gradient-to-br from-blue-500 to-purple-600 rounded-full flex items-center justify-center text-white font-semibold">
                                                    {user.name.as_ref()
                                                        .and_then(|n| n.chars().next())
                                                        .unwrap_or_else(|| user.id.chars().next().unwrap_or('?'))
                                                        .to_uppercase().to_string()}
                                                </div>
                                            </div>
                                            <div class="ml-4">
                                                <div class="text-sm font-medium text-gray-900 dark:text-gray-100">
                                                    {user.name.as_deref().unwrap_or(&user.id)}
                                                    {if is_current_user {
                                                        html! { <span class="ml-2 text-xs text-gray-500">{"(You)"}</span> }
                                                    } else {
                                                        html! {}
                                                    }}
                                                </div>
                                                <div class="text-sm text-gray-500 dark:text-gray-400">
                                                    {&user.id}
                                                </div>
                                            </div>
                                        </div>
                                    </td>
                                    <td class="px-6 py-4 whitespace-nowrap">
                                        <StatusBadge enabled={user.enabled} />
                                    </td>
                                    <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500 dark:text-gray-400">
                                        {user.created_at.format("%Y-%m-%d").to_string()}
                                    </td>
                                    <td class="px-6 py-4 whitespace-nowrap text-right text-sm font-medium">
                                        <div class="flex items-center justify-end space-x-2">
                                            <button
                                                onclick={on_view}
                                                class="text-blue-600 hover:text-blue-900 dark:text-blue-400 dark:hover:text-blue-300"
                                            >
                                                {"View"}
                                            </button>
                                            {if props.can_manage_users && !is_current_user {
                                                html! {
                                                    <>
                                                        <span class="text-gray-300 dark:text-gray-600">{"|"}</span>
                                                        <button
                                                            onclick={on_toggle}
                                                            class="text-yellow-600 hover:text-yellow-900 dark:text-yellow-400 dark:hover:text-yellow-300"
                                                        >
                                                            {if user.enabled { "Disable" } else { "Enable" }}
                                                        </button>
                                                        <span class="text-gray-300 dark:text-gray-600">{"|"}</span>
                                                        <button
                                                            onclick={on_delete}
                                                            class="text-red-600 hover:text-red-900 dark:text-red-400 dark:hover:text-red-300"
                                                        >
                                                            {"Delete"}
                                                        </button>
                                                    </>
                                                }
                                            } else {
                                                html! {}
                                            }}
                                        </div>
                                    </td>
                                </tr>
                            }
                        }).collect::<Html>()}
                    </tbody>
                </table>
            </div>
        </div>
    }
}
