use crate::utils::is_tauri;
use gate_frontend_common::{
    components::{Onboarding, Spinner as LoadingSpinner},
    hooks::{use_webauthn, WebAuthnState},
    services::{BootstrapService, BootstrapStatus},
};
use wasm_bindgen::prelude::*;
use yew::prelude::*;

#[function_component(LocalAuth)]
pub fn local_auth() -> Html {
    let webauthn = use_webauthn();
    let has_tried_auto_auth = use_state(|| false);
    let bootstrap_status = use_state(|| Option::<BootstrapStatus>::None);
    let bootstrap_token = use_state(|| Option::<String>::None);
    let show_onboarding = use_state(|| false);

    let bootstrap_service = use_memo((), |_| BootstrapService::new());

    // Check bootstrap status on mount
    {
        let bootstrap_service = bootstrap_service.clone();
        let bootstrap_status = bootstrap_status.clone();
        let show_onboarding = show_onboarding.clone();

        use_effect_with((), move |_| {
            wasm_bindgen_futures::spawn_local(async move {
                if let Ok(status) = bootstrap_service.check_status().await {
                    // If bootstrap is needed, show onboarding
                    if status.needs_bootstrap {
                        show_onboarding.set(true);
                    }
                    bootstrap_status.set(Some(status));
                }
            });
            || ()
        });
    }

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

    // Fetch bootstrap token when needed for Tauri
    {
        let bootstrap_token = bootstrap_token.clone();
        let show_onboarding = show_onboarding.clone();

        use_effect_with(*show_onboarding, move |show| {
            if *show && is_tauri() {
                wasm_bindgen_futures::spawn_local(async move {
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
                    if !result.is_null() && !result.is_undefined() {
                        if let Some(token) = result.as_string() {
                            bootstrap_token.set(Some(token));
                        }
                    }
                });
            }
            || ()
        });
    }

    // Handle onboarding completion
    let on_complete = {
        let show_onboarding = show_onboarding.clone();
        Callback::from(move |_| {
            show_onboarding.set(false);
            // Refresh the page to show authenticated state
            let window = web_sys::window().unwrap();
            let location = window.location();
            location.reload().ok();
        })
    };

    // Clear error and retry authentication
    let on_retry_auth = {
        let webauthn = webauthn.clone();
        Callback::from(move |_| {
            webauthn.clear_error();
            webauthn.authenticate();
        })
    };

    html! {
        <>
            {if *show_onboarding && (*bootstrap_token).is_some() {
                html! {
                    <Onboarding
                        bootstrap_token={(*bootstrap_token).as_ref().unwrap().clone()}
                        on_complete={on_complete}
                        bootstrap_status={(*bootstrap_status).clone()}
                    />
                }
            } else {
                match webauthn.state() {
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

                                <button
                                    class="w-full px-4 py-3 bg-white/10 hover:bg-white/20 text-white rounded-lg font-medium transition-all border border-white/20"
                                    onclick={on_retry_auth}
                                >
                                    {"Try Again"}
                                </button>

                                {if let Some(status) = (*bootstrap_status).as_ref() {
                                    if status.needs_bootstrap {
                                        html! {
                                            <div class="mt-4 p-4 bg-amber-500/20 border border-amber-500/30 rounded-lg text-center">
                                                <p class="text-amber-200 text-sm">
                                                    {"No users registered yet. Please use the bootstrap token to create the first admin account."}
                                                </p>
                                            </div>
                                        }
                                    } else {
                                        html! {}
                                    }
                                } else {
                                    html! {}
                                }}
                            </div>
                        }
                    }
                    WebAuthnState::Idle => {
                        html! {
                            <div class="text-center">
                                <p class="text-white/70">{"Ready to authenticate"}</p>
                            </div>
                        }
                    }
                }
            }}
        </>
    }
}
