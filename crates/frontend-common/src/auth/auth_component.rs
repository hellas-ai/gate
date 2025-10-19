//! Function component wrapper for Auth

use crate::auth::{use_auth, use_is_authenticated, AuthAction};
use crate::components::Spinner as LoadingSpinner;
use crate::hooks::{use_webauthn, WebAuthnState};
use yew::prelude::*;

#[function_component(AuthComponent)]
pub fn auth_component() -> Html {
    let auth = use_auth();
    let webauthn = use_webauthn();
    let is_authenticated = use_is_authenticated();

    let name = use_state(String::new);
    let device_name = use_state(String::new);

    // Handle input changes
    let on_name_input = {
        let name = name.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            name.set(input.value());
        })
    };

    let on_device_input = {
        let device_name = device_name.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            device_name.set(input.value());
        })
    };

    // Create state for triggering actions
    let trigger_register = use_state(|| false);
    let trigger_login = use_state(|| false);
    let trigger_clear_error = use_state(|| false);

    // Effect for registration
    {
        let webauthn = webauthn.clone();
        let name = name.clone();
        let device_name = device_name.clone();
        let trigger_register = trigger_register.clone();

        use_effect_with(trigger_register.clone(), move |trigger| {
            if **trigger {
                let name_value = (*name).clone();
                let device_value = if (*device_name).is_empty() {
                    None
                } else {
                    Some((*device_name).clone())
                };
                if !name_value.is_empty() {
                    webauthn.register(name_value, device_value, None);
                }
                trigger_register.set(false);
            }
        });
    }

    // Effect for login
    {
        let webauthn = webauthn.clone();
        let trigger_login = trigger_login.clone();

        use_effect_with(trigger_login.clone(), move |trigger| {
            if **trigger {
                webauthn.authenticate();
                trigger_login.set(false);
            }
        });
    }

    // Effect for clearing error
    {
        let webauthn = webauthn.clone();
        let trigger_clear_error = trigger_clear_error.clone();

        use_effect_with(trigger_clear_error.clone(), move |trigger| {
            if **trigger {
                webauthn.clear_error();
                trigger_clear_error.set(false);
            }
        });
    }

    // Handle registration
    let on_register = {
        let trigger_register = trigger_register.clone();
        Callback::from(move |_| {
            trigger_register.set(true);
        })
    };

    // Handle login
    let on_login = {
        let trigger_login = trigger_login.clone();
        Callback::from(move |_| {
            trigger_login.set(true);
        })
    };

    // Handle logout
    let on_logout = {
        let auth = auth.clone();
        Callback::from(move |_| {
            auth.dispatch(AuthAction::Logout);
        })
    };

    // Clear error
    let on_clear_error = {
        let trigger_clear_error = trigger_clear_error.clone();
        Callback::from(move |_| {
            trigger_clear_error.set(true);
        })
    };

    // Check if already authenticated
    if is_authenticated {
        if let crate::auth::context::AuthState::Authenticated { name, .. } = &auth.state {
            return html! {
                <div class="max-w-md mx-auto p-6">
                    <div class="bg-green-50 dark:bg-green-900 border border-green-200 dark:border-green-700 rounded-lg p-6 text-center">
                        <h3 class="text-xl font-semibold text-green-800 dark:text-green-200 mb-2">{"✅ Authenticated"}</h3>
                        <p class="text-gray-700 dark:text-gray-300 mb-4">{format!("Welcome, {}!", name)}</p>
                        <button class="px-4 py-2 bg-gray-200 hover:bg-gray-300 dark:bg-gray-700 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-md transition-colors" onclick={on_logout}>
                            {"Sign Out"}
                        </button>
                    </div>
                </div>
            };
        }
    }

    // Handle WebAuthn state
    match webauthn.state() {
        WebAuthnState::Processing => {
            html! {
                <div class="max-w-md mx-auto p-6">
                    <LoadingSpinner text={Some("Please follow the prompts on your device...".to_string())} />
                    <p class="text-sm text-gray-600 dark:text-gray-400 text-center mt-4">{"You may need to use Touch ID, Face ID, or your security key"}</p>
                </div>
            }
        }
        WebAuthnState::Error(error) => {
            html! {
                <div class="max-w-md mx-auto p-6">
                    <div class="bg-red-50 dark:bg-red-900 border border-red-200 dark:border-red-700 rounded-lg p-6 text-center">
                        <h3 class="text-xl font-semibold text-red-800 dark:text-red-200 mb-2">{"❌ Authentication Error"}</h3>
                        <p class="text-red-700 dark:text-red-300 mb-4">{error}</p>
                        <button class="px-4 py-2 bg-gray-200 hover:bg-gray-300 dark:bg-gray-700 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-md transition-colors" onclick={on_clear_error}>
                            {"Try Again"}
                        </button>
                    </div>
                </div>
            }
        }
        WebAuthnState::Idle => {
            html! {
                <div class="max-w-md mx-auto p-6">
                    <h2 class="text-2xl font-bold text-gray-800 dark:text-gray-200 mb-6 text-center">{"WebAuthn Authentication"}</h2>

                    <div class="bg-white dark:bg-gray-800 rounded-lg shadow-md p-6">
                        <div class="mb-4">
                            <label for="name" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">{"Display Name"}</label>
                            <input
                                type="text"
                                id="name"
                                class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-200 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500"
                                placeholder="Enter your name (e.g., alice)"
                                value={(*name).clone()}
                                oninput={on_name_input}
                            />
                        </div>

                        <div class="mb-6">
                            <label for="device" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">{"Device Name (optional)"}</label>
                            <input
                                type="text"
                                id="device"
                                class="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-200 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500"
                                placeholder="e.g., MacBook Pro"
                                value={(*device_name).clone()}
                                oninput={on_device_input}
                            />
                        </div>

                        <div class="flex flex-col sm:flex-row gap-3">
                            <button
                                class="flex-1 px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white rounded-md transition-colors disabled:bg-gray-300 dark:disabled:bg-gray-600 disabled:cursor-not-allowed"
                                onclick={on_register}
                                disabled={(*name).is_empty()}
                            >
                                {"Register New Device"}
                            </button>
                            <button class="flex-1 px-4 py-2 bg-gray-200 hover:bg-gray-300 dark:bg-gray-700 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-md transition-colors" onclick={on_login}>
                                {"Sign In with Existing Device"}
                            </button>
                        </div>
                    </div>

                    <div class="mt-6 p-4 bg-blue-50 dark:bg-blue-900 border border-blue-200 dark:border-blue-700 rounded-lg">
                        <p class="text-sm text-blue-800 dark:text-blue-200">{"🔐 WebAuthn provides secure, passwordless authentication using your device's biometrics or security key."}</p>
                    </div>
                </div>
            }
        }
    }
}
