//! Bootstrap prompt component wrapper for onboarding

use crate::components::Onboarding;
use crate::services::BootstrapStatus;
use yew::prelude::*;

/// Props for the bootstrap prompt component
#[derive(Properties, PartialEq)]
pub struct BootstrapPromptProps {
    /// Callback when bootstrap token is entered
    pub on_token: Callback<String>,
    /// Whether to show the prompt
    pub show: bool,
    /// Bootstrap status (optional, passed from container)
    #[prop_or_default]
    pub bootstrap_status: Option<BootstrapStatus>,
    /// Bootstrap token (optional, only shown when provided)
    #[prop_or_default]
    pub bootstrap_token: Option<String>,
    /// Whether token is being loaded
    #[prop_or_default]
    pub is_loading_token: bool,
    /// Token loading error
    #[prop_or_default]
    pub token_error: Option<String>,
}

/// Bootstrap prompt component - wrapper for the Onboarding flow
#[function_component(BootstrapPrompt)]
pub fn bootstrap_prompt(props: &BootstrapPromptProps) -> Html {
    // Handle onboarding completion
    let on_complete = {
        Callback::from(move |_| {
            // Redirect to home or refresh
            let window = web_sys::window().unwrap();
            let location = window.location();
            location.set_href("/").ok();
        })
    };

    if !props.show {
        return html! {};
    }

    // If we have a bootstrap token, show the onboarding flow
    if let Some(bootstrap_token) = &props.bootstrap_token {
        html! {
            <Onboarding
                bootstrap_token={bootstrap_token.clone()}
                on_complete={on_complete}
                bootstrap_status={props.bootstrap_status.clone()}
            />
        }
    } else if props.is_loading_token {
        // Show loading state while token is being fetched
        html! {
            <div class="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50">
                <div class="bg-white dark:bg-gray-800 rounded-lg p-6 max-w-md w-full mx-4 shadow-xl">
                    <div class="text-center">
                        <div class="inline-block animate-spin rounded-full h-8 w-8 border-b-2 border-blue-600"></div>
                        <p class="text-gray-600 dark:text-gray-400 mt-4">
                            {"Loading bootstrap token..."}
                        </p>
                    </div>
                </div>
            </div>
        }
    } else {
        // No token available - this shouldn't happen in normal flow
        html! {
            <div class="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50">
                <div class="bg-white dark:bg-gray-800 rounded-lg p-6 max-w-md w-full mx-4 shadow-xl">
                    <h2 class="text-2xl font-bold mb-4 text-gray-800 dark:text-white">
                        {"Bootstrap Required"}
                    </h2>
                    <p class="text-gray-600 dark:text-gray-300">
                        {"A bootstrap token is required to set up the first admin user. Please check the application logs for the bootstrap token."}
                    </p>
                    {if let Some(error) = &props.token_error {
                        html! {
                            <div class="mt-4 p-3 bg-red-50 dark:bg-red-900/20 rounded-lg">
                                <p class="text-sm text-red-800 dark:text-red-200">{error}</p>
                            </div>
                        }
                    } else {
                        html! {}
                    }}
                </div>
            </div>
        }
    }
}
