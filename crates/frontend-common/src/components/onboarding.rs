//! Multi-step onboarding component for initial setup

use crate::client::create_public_client;
use crate::hooks::{use_webauthn, WebAuthnState};
use crate::services::BootstrapStatus;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

/// Daemon status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub node_id: String,
    pub version: String,
    pub is_initialized: bool,
    pub listen_addresses: Vec<String>,
    pub database_path: Option<String>,
    pub config_path: Option<String>,
    pub uptime_seconds: u64,
    pub hostname: String,
}

/// Onboarding step enum
#[derive(Debug, Clone, PartialEq)]
pub enum OnboardingStep {
    DaemonStatus,
    DeviceName,
    UserSetup,
    Complete,
}

/// Props for the onboarding component
#[derive(Properties, PartialEq)]
pub struct OnboardingProps {
    /// Bootstrap token for initial setup
    pub bootstrap_token: String,
    /// Callback when onboarding is complete
    pub on_complete: Callback<()>,
    /// Bootstrap status (optional)
    #[prop_or_default]
    pub bootstrap_status: Option<BootstrapStatus>,
}

/// Onboarding component
#[function_component(Onboarding)]
pub fn onboarding(props: &OnboardingProps) -> Html {
    let current_step = use_state(|| OnboardingStep::DaemonStatus);
    let daemon_status = use_state(|| None::<DaemonStatus>);
    let device_name = use_state(String::new);
    let user_name = use_state(String::new);
    let key_name = use_state(String::new);
    let error = use_state(|| None::<String>);
    let loading = use_state(|| false);
    let webauthn = use_webauthn();

    // Fetch daemon status on mount
    {
        let daemon_status = daemon_status.clone();
        let loading = loading.clone();
        let error = error.clone();
        use_effect_with((), move |_| {
            loading.set(true);
            spawn_local(async move {
                match create_public_client() {
                    Ok(client) => {
                        // Make a direct GET request to /api/status
                        let request = client.request(reqwest::Method::GET, "/api/status");
                        match client.execute::<DaemonStatus>(request).await {
                            Ok(status) => {
                                daemon_status.set(Some(status));
                            }
                            Err(e) => {
                                error.set(Some(format!("Failed to fetch status: {}", e)));
                            }
                        }
                    }
                    Err(e) => {
                        error.set(Some(format!("Failed to create client: {}", e)));
                    }
                }
                loading.set(false);
            });
        });
    }

    // Handle step transitions
    let on_next = {
        let current_step = current_step.clone();
        let device_name = device_name.clone();
        let user_name = user_name.clone();
        let key_name = key_name.clone();
        Callback::from(move |_| match *current_step {
            OnboardingStep::DaemonStatus => {
                current_step.set(OnboardingStep::DeviceName);
            }
            OnboardingStep::DeviceName => {
                if !device_name.is_empty() {
                    current_step.set(OnboardingStep::UserSetup);
                }
            }
            OnboardingStep::UserSetup => {
                if !user_name.is_empty() && !key_name.is_empty() {
                    current_step.set(OnboardingStep::Complete);
                }
            }
            OnboardingStep::Complete => {}
        })
    };

    // Handle final submission
    let on_submit = {
        let webauthn = webauthn.clone();
        let user_name = user_name.clone();
        let key_name = key_name.clone();
        let device_name = device_name.clone();
        let bootstrap_token = props.bootstrap_token.clone();
        let on_complete = props.on_complete.clone();
        let error = error.clone();

        Callback::from(move |e: SubmitEvent| {
            e.prevent_default();

            let name = (*user_name).clone();
            let key = (*key_name).clone();
            let device = (*device_name).clone();
            let token = bootstrap_token.clone();
            let on_complete = on_complete.clone();
            let error = error.clone();

            if !name.is_empty() && !key.is_empty() {
                // Start WebAuthn registration with bootstrap token
                webauthn.register(name.clone(), Some(key), Some(token));

                // Save device name after successful registration
                spawn_local(async move {
                    // Wait a moment for registration to complete
                    gloo::timers::future::TimeoutFuture::new(500).await;

                    // Save device name to config
                    match create_public_client() {
                        Ok(client) => {
                            let request = client
                                .request(reqwest::Method::PATCH, "/api/config/device")
                                .json(&serde_json::json!({ "device_name": device }));
                            match client.execute::<serde_json::Value>(request).await {
                                Ok(_) => {
                                    on_complete.emit(());
                                }
                                Err(e) => {
                                    error.set(Some(format!("Failed to save device name: {}", e)));
                                }
                            }
                        }
                        Err(e) => {
                            error.set(Some(format!("Failed to create client: {}", e)));
                        }
                    }
                });
            }
        })
    };

    // Input handlers
    let on_device_name_input = {
        let device_name = device_name.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            device_name.set(input.value());
        })
    };

    let on_user_name_input = {
        let user_name = user_name.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            user_name.set(input.value());
        })
    };

    let on_key_name_input = {
        let key_name = key_name.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            key_name.set(input.value());
        })
    };

    html! {
        <div class="min-h-screen flex items-center justify-center bg-gradient-to-br from-gray-900 via-blue-900 to-purple-900">
            <div class="max-w-2xl w-full p-8">
                <div class="backdrop-blur-lg bg-white/10 rounded-2xl shadow-2xl p-8 border border-white/20">
                    {match *current_step {
                        OnboardingStep::DaemonStatus => {
                            html! {
                                <>
                                    <div class="text-center mb-8">
                                        <h1 class="text-3xl font-bold text-white mb-2">{"Welcome to Gate"}</h1>
                                        <p class="text-white/70">{"Your secure AI gateway hasn't been initialized yet"}</p>
                                    </div>

                                    {if let Some(status) = (*daemon_status).as_ref() {
                                        html! {
                                            <div class="space-y-4 mb-8">
                                                <div class="bg-black/20 rounded-lg p-4">
                                                    <h3 class="text-sm font-semibold text-white/80 mb-3 uppercase tracking-wider">{"Daemon Information"}</h3>
                                                    <dl class="space-y-2">
                                                        <div class="flex justify-between">
                                                            <dt class="text-white/60 text-sm">{"Node ID"}</dt>
                                                            <dd class="text-white font-mono text-sm">{&status.node_id[..16]}{"..."}</dd>
                                                        </div>
                                                        <div class="flex justify-between">
                                                            <dt class="text-white/60 text-sm">{"Version"}</dt>
                                                            <dd class="text-white text-sm">{&status.version}</dd>
                                                        </div>
                                                        <div class="flex justify-between">
                                                            <dt class="text-white/60 text-sm">{"Hostname"}</dt>
                                                            <dd class="text-white text-sm">{&status.hostname}</dd>
                                                        </div>
                                                        {if let Some(db_path) = &status.database_path {
                                                            html! {
                                                                <div class="flex justify-between">
                                                                    <dt class="text-white/60 text-sm">{"Database"}</dt>
                                                                    <dd class="text-white text-sm font-mono text-xs" title={db_path.clone()}>
                                                                        {"..."}{&db_path[db_path.len().saturating_sub(30)..]}
                                                                    </dd>
                                                                </div>
                                                            }
                                                        } else {
                                                            html! {}
                                                        }}
                                                        <div class="flex justify-between">
                                                            <dt class="text-white/60 text-sm">{"Listen Address"}</dt>
                                                            <dd class="text-white text-sm">{&status.listen_addresses[0]}</dd>
                                                        </div>
                                                    </dl>
                                                </div>

                                                <button
                                                    onclick={on_next.clone()}
                                                    class="w-full px-4 py-3 bg-gradient-to-r from-blue-500 to-purple-600 hover:from-blue-600 hover:to-purple-700 text-white rounded-lg font-medium transition-all"
                                                >
                                                    {"Initialize"}
                                                </button>
                                            </div>
                                        }
                                    } else if *loading {
                                        html! {
                                            <div class="text-center py-8">
                                                <div class="inline-block animate-spin rounded-full h-8 w-8 border-b-2 border-white"></div>
                                                <p class="text-white/70 mt-4">{"Loading daemon status..."}</p>
                                            </div>
                                        }
                                    } else {
                                        html! {
                                            <div class="text-center py-8">
                                                <p class="text-white/70">{"Unable to fetch daemon status"}</p>
                                            </div>
                                        }
                                    }}
                                </>
                            }
                        }
                        OnboardingStep::DeviceName => {
                            html! {
                                <>
                                    <div class="mb-8">
                                        <div class="flex items-center mb-4">
                                            <div class="w-8 h-8 rounded-full bg-blue-500 text-white flex items-center justify-center text-sm font-bold mr-3">{"1"}</div>
                                            <h2 class="text-2xl font-bold text-white">{"Name Your Device"}</h2>
                                        </div>
                                        <p class="text-white/70 ml-11">{"Give this Gate instance a friendly name to identify it"}</p>
                                    </div>

                                    <div class="space-y-4">
                                        <div>
                                            <label class="block text-white/80 text-sm font-medium mb-2">
                                                {"Device Name"}
                                            </label>
                                            <input
                                                type="text"
                                                class="w-full px-4 py-3 bg-white/10 border border-white/20 rounded-lg text-white placeholder-white/50 focus:outline-none focus:border-blue-400 focus:bg-white/20 transition-all"
                                                placeholder="e.g., MacBook Pro, Home Server"
                                                value={(*device_name).clone()}
                                                oninput={on_device_name_input}
                                            />
                                            <p class="text-white/50 text-xs mt-2">
                                                {"This helps you identify this Gate instance when managing multiple devices"}
                                            </p>
                                        </div>

                                        <button
                                            onclick={on_next.clone()}
                                            disabled={device_name.is_empty()}
                                            class="w-full px-4 py-3 bg-gradient-to-r from-blue-500 to-purple-600 hover:from-blue-600 hover:to-purple-700 text-white rounded-lg font-medium transition-all disabled:opacity-50 disabled:cursor-not-allowed"
                                        >
                                            {"Continue"}
                                        </button>
                                    </div>
                                </>
                            }
                        }
                        OnboardingStep::UserSetup => {
                            html! {
                                <>
                                    <div class="mb-8">
                                        <div class="flex items-center mb-4">
                                            <div class="w-8 h-8 rounded-full bg-blue-500 text-white flex items-center justify-center text-sm font-bold mr-3">{"2"}</div>
                                            <h2 class="text-2xl font-bold text-white">{"Create Admin Account"}</h2>
                                        </div>
                                        <p class="text-white/70 ml-11">{"Set up your identity and security key"}</p>
                                    </div>

                                    <form onsubmit={on_submit.clone()} class="space-y-4">
                                        <div>
                                            <label class="block text-white/80 text-sm font-medium mb-2">
                                                {"Your Name"}
                                            </label>
                                            <input
                                                type="text"
                                                class="w-full px-4 py-3 bg-white/10 border border-white/20 rounded-lg text-white placeholder-white/50 focus:outline-none focus:border-blue-400 focus:bg-white/20 transition-all"
                                                placeholder="Enter your name"
                                                value={(*user_name).clone()}
                                                oninput={on_user_name_input}
                                                required=true
                                            />
                                            <p class="text-white/50 text-xs mt-2">
                                                {"This will be displayed when you log in"}
                                            </p>
                                        </div>

                                        <div>
                                            <label class="block text-white/80 text-sm font-medium mb-2">
                                                {"Security Key Name"}
                                            </label>
                                            <input
                                                type="text"
                                                class="w-full px-4 py-3 bg-white/10 border border-white/20 rounded-lg text-white placeholder-white/50 focus:outline-none focus:border-blue-400 focus:bg-white/20 transition-all"
                                                placeholder="e.g., YubiKey, Touch ID, Windows Hello"
                                                value={(*key_name).clone()}
                                                oninput={on_key_name_input}
                                                required=true
                                            />
                                            <p class="text-white/50 text-xs mt-2">
                                                {"Name for the biometric or security key you'll use to authenticate"}
                                            </p>
                                        </div>

                                        <button
                                            type="submit"
                                            disabled={user_name.is_empty() || key_name.is_empty()}
                                            class="w-full px-4 py-3 bg-gradient-to-r from-blue-500 to-purple-600 hover:from-blue-600 hover:to-purple-700 text-white rounded-lg font-medium transition-all disabled:opacity-50 disabled:cursor-not-allowed"
                                        >
                                            {"Continue"}
                                        </button>
                                    </form>
                                </>
                            }
                        }
                        OnboardingStep::Complete => {
                            html! {
                                <>
                                    <div class="mb-8">
                                        <div class="flex items-center mb-4">
                                            <div class="w-8 h-8 rounded-full bg-green-500 text-white flex items-center justify-center text-sm font-bold mr-3">{"✓"}</div>
                                            <h2 class="text-2xl font-bold text-white">{"Ready to Start"}</h2>
                                        </div>
                                        <p class="text-white/70 ml-11">{"Your Gate instance is configured and ready"}</p>
                                    </div>

                                    {match webauthn.state() {
                                        WebAuthnState::Processing => html! {
                                            <div class="text-center py-8">
                                                <div class="inline-block animate-spin rounded-full h-8 w-8 border-b-2 border-white mb-4"></div>
                                                <p class="text-white">{"Creating your account..."}</p>
                                                <p class="text-sm text-white/70 mt-4">
                                                    {"Use your device's biometrics or security key"}
                                                </p>
                                            </div>
                                        },
                                        WebAuthnState::Error(err) => html! {
                                            <div class="space-y-4">
                                                <div class="bg-red-500/20 border border-red-500/30 rounded-lg p-4">
                                                    <p class="text-red-200 text-sm">{err}</p>
                                                </div>
                                                <button
                                                    onclick={Callback::from({
                                                        let on_submit = on_submit.clone();
                                                        move |_e: MouseEvent| {
                                                            // Create a dummy form submit event
                                                            let window = web_sys::window().unwrap();
                                                            let document = window.document().unwrap();
                                                            let _form = document.create_element("form").unwrap();
                                                            let event = web_sys::Event::new("submit").unwrap();
                                                            on_submit.emit(event.unchecked_into());
                                                        }
                                                    })}
                                                    class="w-full px-4 py-3 bg-gradient-to-r from-blue-500 to-purple-600 hover:from-blue-600 hover:to-purple-700 text-white rounded-lg font-medium transition-all"
                                                >
                                                    {"Try Again"}
                                                </button>
                                            </div>
                                        },
                                        WebAuthnState::Idle => html! {
                                            <div class="space-y-4">
                                                <div class="bg-black/20 rounded-lg p-4">
                                                    <h3 class="text-sm font-semibold text-white/80 mb-3">{"Configuration Summary"}</h3>
                                                    <dl class="space-y-2">
                                                        <div class="flex justify-between">
                                                            <dt class="text-white/60 text-sm">{"Device"}</dt>
                                                            <dd class="text-white text-sm">{(*device_name).clone()}</dd>
                                                        </div>
                                                        <div class="flex justify-between">
                                                            <dt class="text-white/60 text-sm">{"Admin"}</dt>
                                                            <dd class="text-white text-sm">{(*user_name).clone()}</dd>
                                                        </div>
                                                        <div class="flex justify-between">
                                                            <dt class="text-white/60 text-sm">{"Key"}</dt>
                                                            <dd class="text-white text-sm">{(*key_name).clone()}</dd>
                                                        </div>
                                                    </dl>
                                                </div>

                                                <button
                                                    onclick={Callback::from({
                                                        let on_submit = on_submit.clone();
                                                        move |_e: MouseEvent| {
                                                            // Create a dummy form submit event
                                                            let window = web_sys::window().unwrap();
                                                            let document = window.document().unwrap();
                                                            let _form = document.create_element("form").unwrap();
                                                            let event = web_sys::Event::new("submit").unwrap();
                                                            on_submit.emit(event.unchecked_into());
                                                        }
                                                    })}
                                                    class="w-full px-4 py-3 bg-gradient-to-r from-green-500 to-blue-600 hover:from-green-600 hover:to-blue-700 text-white rounded-lg font-medium transition-all"
                                                >
                                                    {"Start Using Gate"}
                                                </button>
                                            </div>
                                        }
                                    }}
                                </>
                            }
                        }
                    }}

                    {if let Some(err) = (*error).as_ref() {
                        html! {
                            <div class="mt-4 bg-red-500/20 border border-red-500/30 rounded-lg p-4">
                                <p class="text-red-200 text-sm">{err}</p>
                            </div>
                        }
                    } else {
                        html! {}
                    }}

                    {if let Some(status) = &props.bootstrap_status {
                        html! {
                            <div class="mt-4 p-4 bg-blue-50 dark:bg-blue-900/20 rounded-lg">
                                <p class="text-sm text-blue-800 dark:text-blue-200">
                                    {&status.message}
                                </p>
                            </div>
                        }
                    } else {
                        html! {}
                    }}
                </div>
            </div>
        </div>
    }
}
