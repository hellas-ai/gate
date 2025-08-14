//! Shared UI components for user management

use crate::services::user::UserInfo;
use yew::prelude::*;

/// Status badge component
#[derive(Properties, PartialEq)]
pub struct StatusBadgeProps {
    pub enabled: bool,
}

#[function_component(StatusBadge)]
pub fn status_badge(props: &StatusBadgeProps) -> Html {
    let (class, text) = if props.enabled {
        (
            "inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400",
            "Active"
        )
    } else {
        (
            "inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium bg-red-100 text-red-800 dark:bg-red-900/30 dark:text-red-400",
            "Disabled"
        )
    };

    html! {
        <span class={class}>
            {text}
        </span>
    }
}

/// User card component for consistent display
#[derive(Properties, PartialEq)]
pub struct UserCardProps {
    pub user: UserInfo,
    pub on_click: Option<Callback<String>>,
    pub compact: bool,
}

#[function_component(UserCard)]
pub fn user_card(props: &UserCardProps) -> Html {
    let on_click = {
        let user_id = props.user.id.clone();
        let callback = props.on_click.clone();
        Callback::from(move |e: MouseEvent| {
            e.prevent_default();
            if let Some(cb) = &callback {
                cb.emit(user_id.clone());
            }
        })
    };

    if props.compact {
        html! {
            <div
                class="flex items-center justify-between p-3 hover:bg-gray-50 dark:hover:bg-gray-800 cursor-pointer rounded-lg transition-colors"
                onclick={on_click}
            >
                <div class="flex items-center space-x-3">
                    <div class="flex-shrink-0">
                        <div class="w-10 h-10 bg-gradient-to-br from-blue-500 to-purple-600 rounded-full flex items-center justify-center text-white font-semibold">
                            {props.user.name.as_ref()
                                .and_then(|n| n.chars().next())
                                .unwrap_or_else(|| props.user.id.chars().next().unwrap_or('?'))
                                .to_uppercase().to_string()}
                        </div>
                    </div>
                    <div>
                        <p class="text-sm font-medium text-gray-900 dark:text-gray-100">
                            {props.user.name.as_deref().unwrap_or(&props.user.id)}
                        </p>
                        <p class="text-xs text-gray-500 dark:text-gray-400">
                            {&props.user.id}
                        </p>
                    </div>
                </div>
                <StatusBadge enabled={props.user.enabled} />
            </div>
        }
    } else {
        html! {
            <div
                class="bg-white dark:bg-gray-800 rounded-lg shadow p-6 hover:shadow-md transition-shadow cursor-pointer"
                onclick={on_click}
            >
                <div class="flex items-start justify-between">
                    <div class="flex items-center space-x-4">
                        <div class="flex-shrink-0">
                            <div class="w-12 h-12 bg-gradient-to-br from-blue-500 to-purple-600 rounded-full flex items-center justify-center text-white font-semibold text-lg">
                                {props.user.name.as_ref()
                                    .and_then(|n| n.chars().next())
                                    .unwrap_or_else(|| props.user.id.chars().next().unwrap_or('?'))
                                    .to_uppercase().to_string()}
                            </div>
                        </div>
                        <div>
                            <h3 class="text-lg font-medium text-gray-900 dark:text-gray-100">
                                {props.user.name.as_deref().unwrap_or(&props.user.id)}
                            </h3>
                            <p class="text-sm text-gray-500 dark:text-gray-400">
                                {&props.user.id}
                            </p>
                            <p class="text-xs text-gray-400 dark:text-gray-500 mt-1">
                                {format!("Created: {}", props.user.created_at.format("%Y-%m-%d"))}
                            </p>
                        </div>
                    </div>
                    <StatusBadge enabled={props.user.enabled} />
                </div>
            </div>
        }
    }
}

/// Action button with permission awareness
#[derive(Properties, PartialEq)]
pub struct ActionButtonProps {
    pub label: String,
    pub icon: Html,
    pub on_click: Callback<()>,
    pub enabled: bool,
    pub variant: ActionButtonVariant,
    #[prop_or_default]
    pub tooltip: Option<String>,
}

#[derive(Clone, PartialEq)]
pub enum ActionButtonVariant {
    Primary,
    Danger,
}

#[function_component(ActionButton)]
pub fn action_button(props: &ActionButtonProps) -> Html {
    let on_click = {
        let callback = props.on_click.clone();
        Callback::from(move |e: MouseEvent| {
            e.prevent_default();
            callback.emit(());
        })
    };

    let base_class = "inline-flex items-center px-3 py-2 border text-sm font-medium rounded-md transition-colors";
    let variant_class = match props.variant {
        ActionButtonVariant::Primary => {
            if props.enabled {
                "border-transparent bg-blue-600 text-white hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-blue-500"
            } else {
                "border-gray-300 bg-gray-100 text-gray-400 cursor-not-allowed"
            }
        }
        ActionButtonVariant::Danger => {
            if props.enabled {
                "border-transparent bg-red-600 text-white hover:bg-red-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-red-500"
            } else {
                "border-gray-300 bg-gray-100 text-gray-400 cursor-not-allowed"
            }
        }
    };

    html! {
        <button
            class={format!("{} {}", base_class, variant_class)}
            onclick={on_click}
            disabled={!props.enabled}
            title={props.tooltip.clone()}
        >
            <span class="mr-2">{props.icon.clone()}</span>
            {&props.label}
        </button>
    }
}

/// Empty state component
#[derive(Properties, PartialEq)]
pub struct EmptyStateProps {
    pub title: String,
    pub description: String,
    pub icon: Html,
    #[prop_or_default]
    pub action: Option<Html>,
}

#[function_component(EmptyState)]
pub fn empty_state(props: &EmptyStateProps) -> Html {
    html! {
        <div class="text-center py-12">
            <div class="flex justify-center mb-4">
                <div class="w-12 h-12 text-gray-400">
                    {props.icon.clone()}
                </div>
            </div>
            <h3 class="mt-2 text-sm font-medium text-gray-900 dark:text-gray-100">
                {&props.title}
            </h3>
            <p class="mt-1 text-sm text-gray-500 dark:text-gray-400">
                {&props.description}
            </p>
            if let Some(action) = &props.action {
                <div class="mt-6">
                    {action.clone()}
                </div>
            }
        </div>
    }
}

/// Loading skeleton component
#[function_component(UserListSkeleton)]
pub fn user_list_skeleton() -> Html {
    html! {
        <div class="space-y-3 animate-pulse">
            {(0..5).map(|_| html! {
                <div class="bg-white dark:bg-gray-800 rounded-lg p-4">
                    <div class="flex items-center space-x-3">
                        <div class="w-10 h-10 bg-gray-200 dark:bg-gray-700 rounded-full"></div>
                        <div class="flex-1">
                            <div class="h-4 bg-gray-200 dark:bg-gray-700 rounded w-1/4 mb-2"></div>
                            <div class="h-3 bg-gray-200 dark:bg-gray-700 rounded w-1/3"></div>
                        </div>
                        <div class="h-6 bg-gray-200 dark:bg-gray-700 rounded-full w-16"></div>
                    </div>
                </div>
            }).collect::<Html>()}
        </div>
    }
}
