use crate::components::{ConfigEditor, UserManagement};
use crate::local_auth::LocalAuth;
use gate_frontend_common::{
    auth::{use_auth, use_is_authenticated, AuthAction, AuthProvider},
    components::{LiveChat, ThemeToggle},
    theme::ThemeProvider,
};
use yew::prelude::*;

#[function_component(LocalApp)]
pub fn local_app() -> Html {
    html! {
        <ThemeProvider>
            <AuthProvider>
                <LocalAppContent />
            </AuthProvider>
        </ThemeProvider>
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Chat,
    Config,
    Users,
}

#[function_component(LocalAppContent)]
fn local_app_content() -> Html {
    let auth = use_auth();
    let is_authenticated = use_is_authenticated();
    let active_tab = use_state(|| Tab::Chat);
    let is_admin = use_state(|| false);

    let on_tab_change = {
        let active_tab = active_tab.clone();
        Callback::from(move |tab: Tab| {
            active_tab.set(tab);
        })
    };

    let on_logout = {
        let auth = auth.clone();
        Callback::from(move |_| {
            auth.dispatch(AuthAction::Logout);
        })
    };

    // For now, treat all authenticated users as having admin access
    // TODO: In the future, check actual permissions from the backend
    {
        let is_admin = is_admin.clone();
        use_effect_with(is_authenticated, move |authenticated| {
            if *authenticated {
                is_admin.set(true);
            }
            || ()
        });
    }

    // Show loading state while auth is being restored from sessionStorage
    if auth.is_loading {
        html! {
            <div class="min-h-screen bg-gradient-to-br from-gray-900 via-blue-900 to-purple-900 flex items-center justify-center">
                <div class="text-center">
                    <div class="inline-flex items-center justify-center w-20 h-20 bg-gradient-to-br from-blue-500 to-purple-600 rounded-full mb-4 shadow-lg animate-pulse">
                        <svg class="w-10 h-10 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z"></path>
                        </svg>
                    </div>
                    <p class="text-white text-lg">{"Loading..."}</p>
                </div>
            </div>
        }
    } else if is_authenticated {
        // Show tabbed interface when authenticated
        html! {
            <div class="h-screen flex flex-col bg-white dark:bg-gray-900">
                <div class="bg-gray-50 dark:bg-gray-800 border-b border-gray-200 dark:border-gray-700">
                    <div class="p-4 flex justify-between items-center">
                        <div class="flex items-center gap-3">
                            <h1 class="text-xl font-bold bg-gradient-to-r from-blue-600 to-purple-600 bg-clip-text text-transparent">
                                {"Gate"}
                            </h1>
                            <span class="text-sm text-gray-500 dark:text-gray-400">{"Local Daemon"}</span>
                        </div>
                        <div class="flex items-center gap-3">
                            <ThemeToggle />
                            <button
                                onclick={on_logout}
                                class="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-lg transition-colors flex items-center gap-2"
                            >
                                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 16l4-4m0 0l-4-4m4 4H7m6 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h4a3 3 0 013 3v1"></path>
                                </svg>
                                {"Logout"}
                            </button>
                        </div>
                    </div>

                    // Tab navigation
                    <div class="flex">
                        <button
                            class={format!("px-6 py-3 text-sm font-medium transition-colors {}",
                                if *active_tab == Tab::Chat {
                                    "text-blue-600 dark:text-blue-400 border-b-2 border-blue-600 dark:border-blue-400"
                                } else {
                                    "text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100"
                                }
                            )}
                            onclick={on_tab_change.reform(|_| Tab::Chat)}
                        >
                            <div class="flex items-center gap-2">
                                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z"></path>
                                </svg>
                                {"Chat"}
                            </div>
                        </button>
                        <button
                            class={format!("px-6 py-3 text-sm font-medium transition-colors {}",
                                if *active_tab == Tab::Config {
                                    "text-blue-600 dark:text-blue-400 border-b-2 border-blue-600 dark:border-blue-400"
                                } else {
                                    "text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100"
                                }
                            )}
                            onclick={on_tab_change.reform(|_| Tab::Config)}
                        >
                            <div class="flex items-center gap-2">
                                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"></path>
                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"></path>
                                </svg>
                                {"Config"}
                            </div>
                        </button>
                        {if *is_admin {
                            html! {
                                <button
                                    class={format!("px-6 py-3 text-sm font-medium transition-colors {}",
                                        if *active_tab == Tab::Users {
                                            "text-blue-600 dark:text-blue-400 border-b-2 border-blue-600 dark:border-blue-400"
                                        } else {
                                            "text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100"
                                        }
                                    )}
                                    onclick={on_tab_change.reform(|_| Tab::Users)}
                                >
                                    <div class="flex items-center gap-2">
                                        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4.354a4 4 0 110 5.292M15 21H3v-1a6 6 0 0112 0v1zm0 0h6v-1a6 6 0 00-9-5.197M13 7a4 4 0 11-8 0 4 4 0 018 0z"></path>
                                        </svg>
                                        {"Users"}
                                    </div>
                                </button>
                            }
                        } else {
                            html! {}
                        }}
                    </div>
                </div>

                // Tab content
                <div class="flex-1 overflow-hidden">
                    {match *active_tab {
                        Tab::Chat => html! { <LiveChat /> },
                        Tab::Config => html! { <ConfigEditor /> },
                        Tab::Users => html! { <UserManagement /> },
                    }}
                </div>
            </div>
        }
    } else {
        // Show minimal auth screen
        html! {
            <div class="min-h-screen bg-gradient-to-br from-gray-900 via-blue-900 to-purple-900 flex items-center justify-center px-4">
                <div class="max-w-md w-full">
                    <div class="text-center mb-8">
                        <div class="inline-flex items-center justify-center w-20 h-20 bg-gradient-to-br from-blue-500 to-purple-600 rounded-full mb-4 shadow-lg animate-pulse">
                            <svg class="w-10 h-10 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z"></path>
                            </svg>
                        </div>
                        <h1 class="text-3xl font-bold text-white mb-2">
                            {"Gate Local"}
                        </h1>
                        <p class="text-blue-200">{"Secure AI Gateway"}</p>
                    </div>
                    <div class="bg-white/10 backdrop-blur-md rounded-2xl shadow-2xl p-8 border border-white/20">
                        <LocalAuth />
                    </div>
                </div>
            </div>
        }
    }
}
