use crate::utils::is_tauri;
use gate_frontend_common::{
    components::{BootstrapPrompt, Spinner as LoadingSpinner},
    hooks::{use_webauthn, WebAuthnState},
    services::{BootstrapService, BootstrapStatus},
};
use wasm_bindgen::prelude::*;
use yew::prelude::*;

#[function_component(LocalAuth)]
pub fn local_auth() -> Html {
    let webauthn = use_webauthn();

    let name = use_state(String::new);
    let has_tried_auto_auth = use_state(|| false);
    let bootstrap_status = use_state(|| Option::<BootstrapStatus>::None);
    let bootstrap_token = use_state(|| Option::<String>::None);
    let show_bootstrap_prompt = use_state(|| false);
    let fetched_token = use_state(|| Option::<String>::None);
    let is_loading_token = use_state(|| false);

    let bootstrap_service = use_memo((), |_| BootstrapService::new());

    // Check bootstrap status on mount
    {
        let bootstrap_service = bootstrap_service.clone();
        let bootstrap_status = bootstrap_status.clone();

        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(status) = bootstrap_service.check_status().await {
                    bootstrap_status.set(Some(status));
                }
            });
            || ()
        });
    }

    // Handle input changes
    let on_name_input = {
        let name = name.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            name.set(input.value());
        })
    };

    // Auto-trigger authentication on mount (only once)
    {
        let webauthn = webauthn.clone();
        let has_tried_auto_auth = has_tried_auto_auth.clone();
        let bootstrap_status = bootstrap_status.clone();

        use_effect_with((), move |_| {
            if !*has_tried_auto_auth {
                has_tried_auto_auth.set(true);
                // Only auto-authenticate if bootstrap is complete
                if let Some(status) = (*bootstrap_status).as_ref() {
                    if !status.needs_bootstrap {
                        webauthn.authenticate();
                    }
                } else {
                    // If we don't know bootstrap status yet, try anyway
                    webauthn.authenticate();
                }
            }
        });
    }

    // Fetch bootstrap token when needed
    {
        let fetched_token = fetched_token.clone();
        let is_loading_token = is_loading_token.clone();
        let show_bootstrap_prompt = show_bootstrap_prompt.clone();

        use_effect_with(*show_bootstrap_prompt, move |show| {
            if *show && is_tauri() {
                is_loading_token.set(true);
                wasm_bindgen_futures::spawn_local(async move {
                    // Use the same fetch function from onboarding
                    #[wasm_bindgen(inline_js = "
                    export async function call_get_bootstrap_token_local() {
                        try {
                            if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {
                                return await window.__TAURI_INTERNALS__.invoke('get_bootstrap_token');
                            } else if (window.__TAURI__ && window.__TAURI__.invoke) {
                                return await window.__TAURI__.invoke('get_bootstrap_token');
                            } else if (window.__TAURI__ && window.__TAURI__.tauri && window.__TAURI__.tauri.invoke) {
                                return await window.__TAURI__.tauri.invoke('get_bootstrap_token');
                            }
                            return null;
                        } catch (e) {
                            console.error('Failed to get bootstrap token:', e);
                            return null;
                        }
                    }
                    ")]
                    extern "C" {
                        async fn call_get_bootstrap_token_local() -> JsValue;
                    }

                    let result = call_get_bootstrap_token_local().await;
                    if result.is_string() {
                        fetched_token.set(Some(result.as_string().unwrap()));
                    } else if !result.is_null() {
                        // Try to parse as JSON
                        if let Ok(token) = serde_wasm_bindgen::from_value::<Option<String>>(result)
                        {
                            fetched_token.set(token);
                        }
                    }
                    is_loading_token.set(false);
                });
            }
            || ()
        });
    }

    // Handle registration
    let on_register = {
        let webauthn = webauthn.clone();
        let name = name.clone();
        let bootstrap_status = bootstrap_status.clone();
        let bootstrap_token = bootstrap_token.clone();
        let show_bootstrap_prompt = show_bootstrap_prompt.clone();

        Callback::from(move |_| {
            let name_value = (*name).clone();
            if !name_value.is_empty() {
                // Check if bootstrap is needed
                if let Some(status) = (*bootstrap_status).as_ref() {
                    if status.needs_bootstrap && bootstrap_token.is_none() {
                        // Show bootstrap prompt
                        show_bootstrap_prompt.set(true);
                        return;
                    }
                }

                // Proceed with registration
                webauthn.register(name_value, None, (*bootstrap_token).clone());
            }
        })
    };

    // Handle login
    let on_login = {
        let webauthn = webauthn.clone();
        Callback::from(move |_| {
            webauthn.authenticate();
        })
    };

    // Clear error
    let on_clear_error = {
        let webauthn = webauthn.clone();
        Callback::from(move |_| {
            webauthn.clear_error();
        })
    };

    // Handle bootstrap token received
    let on_bootstrap_token = {
        let bootstrap_token = bootstrap_token.clone();
        let show_bootstrap_prompt = show_bootstrap_prompt.clone();
        let webauthn = webauthn.clone();
        let name = name.clone();

        Callback::from(move |token: String| {
            bootstrap_token.set(Some(token.clone()));
            show_bootstrap_prompt.set(false);

            // Proceed with registration
            let name_value = (*name).clone();
            if !name_value.is_empty() {
                webauthn.register(name_value, None, Some(token));
            }
        })
    };

    html! {
        <>
            <BootstrapPrompt
                show={*show_bootstrap_prompt}
                on_token={on_bootstrap_token}
                bootstrap_status={(*bootstrap_status).clone()}
                bootstrap_token={(*fetched_token).clone()}
                is_loading_token={*is_loading_token}
            />

            {match webauthn.state() {
                WebAuthnState::Processing => {
                    html! {
                        <div class="text-center">
                            <LoadingSpinner text={Some("Authenticating...".to_string())} />
                            <p class="text-sm text-white/70 mt-4">
                                {"Use your device's biometrics or security key"}
                            </p>
                        </div>
                    }
                }
                WebAuthnState::Error(error) => {
                    html! {
                        <div class="space-y-4">
                            <div class="bg-red-500/20 border border-red-500/30 rounded-lg p-4 text-center">
                                <p class="text-red-200 text-sm">{error}</p>
                            </div>

                            // Show registration form if authentication failed
                            <div class="space-y-4">
                                <h3 class="text-white text-center font-medium">{"Create New Account"}</h3>

                                {if let Some(status) = (*bootstrap_status).as_ref() {
                                    if status.needs_bootstrap {
                                        html! {
                                            <div class="bg-amber-500/20 border border-amber-500/30 rounded-lg p-4 text-center mb-4">
                                                <p class="text-amber-200 text-sm">
                                                    {"This is the first user registration. You'll need a bootstrap token."}
                                                </p>
                                            </div>
                                        }
                                    } else {
                                        html! {}
                                    }
                                } else {
                                    html! {}
                                }}

                                <input
                                    type="text"
                                    class="w-full px-4 py-3 bg-white/10 border border-white/20 rounded-lg text-white placeholder-white/50 focus:outline-none focus:border-blue-400 focus:bg-white/20 transition-all"
                                    placeholder="Enter your name"
                                    value={(*name).clone()}
                                    oninput={on_name_input}
                                />
                                <div class="flex gap-3">
                                    <button
                                        class="flex-1 px-4 py-3 bg-blue-600 hover:bg-blue-700 text-white rounded-lg font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                                        onclick={on_register}
                                        disabled={(*name).is_empty()}
                                    >
                                        {"Register"}
                                    </button>
                                    <button
                                        class="flex-1 px-4 py-3 bg-white/10 hover:bg-white/20 text-white rounded-lg font-medium transition-all border border-white/20"
                                        onclick={on_clear_error}
                                    >
                                        {"Try Again"}
                                    </button>
                                </div>
                            </div>
                        </div>
                    }
                }
                WebAuthnState::Idle => {
                    html! {
                        <div class="space-y-4">
                            {if let Some(status) = (*bootstrap_status).as_ref() {
                                if status.needs_bootstrap {
                                    html! {
                                        <div class="bg-blue-500/20 border border-blue-500/30 rounded-lg p-4 text-center mb-4">
                                            <p class="text-blue-200 text-sm">
                                                {"Welcome! This is the first time Gate is being set up."}
                                            </p>
                                            <p class="text-blue-200 text-sm mt-2">
                                                {"Please register to become the administrator."}
                                            </p>
                                        </div>
                                    }
                                } else {
                                    html! {
                                        <p class="text-white/70 text-center text-sm">
                                            {"No existing credentials found"}
                                        </p>
                                    }
                                }
                            } else {
                                html! {
                                    <p class="text-white/70 text-center text-sm">
                                        {"No existing credentials found"}
                                    </p>
                                }
                            }}

                            <input
                                type="text"
                                class="w-full px-4 py-3 bg-white/10 border border-white/20 rounded-lg text-white placeholder-white/50 focus:outline-none focus:border-blue-400 focus:bg-white/20 transition-all"
                                placeholder="Enter your name"
                                value={(*name).clone()}
                                oninput={on_name_input}
                            />

                            <div class="flex gap-3">
                                <button
                                    class="flex-1 px-4 py-3 bg-blue-600 hover:bg-blue-700 text-white rounded-lg font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                                    onclick={on_register}
                                    disabled={(*name).is_empty()}
                                >
                                    {"Register New Device"}
                                </button>
                                <button
                                    class="flex-1 px-4 py-3 bg-white/10 hover:bg-white/20 text-white rounded-lg font-medium transition-all border border-white/20"
                                    onclick={on_login}
                                >
                                    {"Try Existing"}
                                </button>
                            </div>
                        </div>
                    }
                }
            }}
        </>
    }
}
