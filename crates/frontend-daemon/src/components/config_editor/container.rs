use crate::hooks::use_config;
use gloo::timers::callback::Timeout;
use yew::prelude::*;

use super::{
    pages::{
        AuthConfigPage, InferenceConfigPage, NetworkConfigPage, ProvidersConfigPage,
        ServerConfigPage,
    },
    sub_nav::{ConfigPage, SubNav},
    types::*,
};

#[function_component(ConfigEditor)]
pub fn config_editor() -> Html {
    let config_service = use_config();
    let config = use_state(GateConfig::default);
    let is_loading = use_state(|| false);
    let is_saving = use_state(|| false);
    let error_message = use_state(|| None::<String>);
    let success_message = use_state(|| None::<String>);
    let active_page = use_state(|| ConfigPage::Server);

    // Return early if no auth client available
    let Some(config_service) = config_service else {
        return html! {
            <div class="p-6 max-w-6xl mx-auto">
                <div class="bg-white dark:bg-gray-800 rounded-lg shadow-lg p-6">
                    <p class="text-gray-600 dark:text-gray-400">{ "Authentication required to access configuration." }</p>
                </div>
            </div>
        };
    };

    {
        let config_service = config_service.clone();
        let config = config.clone();
        let is_loading = is_loading.clone();
        let error_message = error_message.clone();

        use_effect_with((), move |_| {
            is_loading.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                match config_service.get_config().await {
                    Ok(config_json) => match serde_json::from_value::<GateConfig>(config_json) {
                        Ok(loaded_config) => config.set(loaded_config),
                        Err(e) => error_message.set(Some(format!("Failed to parse config: {e}"))),
                    },
                    Err(e) => error_message.set(Some(format!("Failed to load config: {e}"))),
                }
                is_loading.set(false);
            });
        });
    }

    let on_save = {
        let config_service = config_service.clone();
        let config = config.clone();
        let is_saving = is_saving.clone();
        let error_message = error_message.clone();
        let success_message = success_message.clone();

        Callback::from(move |_| {
            is_saving.set(true);
            error_message.set(None);

            let config_json = match serde_json::to_value(&*config) {
                Ok(json) => json,
                Err(e) => {
                    error_message.set(Some(format!("Failed to serialize config: {e}")));
                    is_saving.set(false);
                    return;
                }
            };

            let config_service = config_service.clone();
            let is_saving = is_saving.clone();
            let error_message = error_message.clone();
            let success_message = success_message.clone();

            wasm_bindgen_futures::spawn_local(async move {
                match config_service.update_config(config_json).await {
                    Ok(_) => {
                        success_message.set(Some("Configuration saved successfully!".to_string()));
                        let success_message = success_message.clone();
                        Timeout::new(3000, move || {
                            success_message.set(None);
                        })
                        .forget();
                    }
                    Err(e) => error_message.set(Some(format!("Failed to save config: {e}"))),
                }
                is_saving.set(false);
            });
        })
    };

    let on_server_change = {
        let config = config.clone();
        Callback::from(move |new_server| {
            let mut new_config = (*config).clone();
            new_config.server = new_server;
            config.set(new_config);
        })
    };

    let on_auth_change = {
        let config = config.clone();
        Callback::from(move |new_auth| {
            let mut new_config = (*config).clone();
            new_config.auth = new_auth;
            config.set(new_config);
        })
    };

    let on_providers_change = {
        let config = config.clone();
        Callback::from(move |new_providers| {
            let mut new_config = (*config).clone();
            new_config.providers = new_providers;
            config.set(new_config);
        })
    };

    let on_tlsforward_change = {
        let config = config.clone();
        Callback::from(move |new_tlsforward| {
            let mut new_config = (*config).clone();
            new_config.tlsforward = new_tlsforward;
            config.set(new_config);
        })
    };

    let on_letsencrypt_change = {
        let config = config.clone();
        Callback::from(move |new_letsencrypt| {
            let mut new_config = (*config).clone();
            new_config.letsencrypt = new_letsencrypt;
            config.set(new_config);
        })
    };

    let on_inference_change = {
        let config = config.clone();
        Callback::from(move |new_inference| {
            let mut new_config = (*config).clone();
            new_config.local_inference = new_inference;
            config.set(new_config);
        })
    };

    let on_page_change = {
        let active_page = active_page.clone();
        Callback::from(move |page| {
            active_page.set(page);
        })
    };

    html! {
        <div class="p-6 max-w-6xl mx-auto">
            <div class="bg-white dark:bg-gray-800 rounded-lg shadow-lg">
                <div class="border-b border-gray-200 dark:border-gray-700 px-6 py-4">
                    <h2 class="text-xl font-semibold text-gray-800 dark:text-gray-200">
                        {"Configuration Editor"}
                    </h2>
                    <p class="text-sm text-gray-600 dark:text-gray-400 mt-1">
                        {"Manage Gate configuration settings"}
                    </p>
                </div>

                <div class="p-6">
                    if *is_loading {
                        <div class="flex justify-center items-center h-64">
                            <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500"></div>
                        </div>
                    } else {
                        <>
                            if let Some(error) = (*error_message).as_ref() {
                                <div class="mb-4 p-4 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-md">
                                    <p class="text-red-700 dark:text-red-300">{error}</p>
                                </div>
                            }

                            if let Some(success) = (*success_message).as_ref() {
                                <div class="mb-4 p-4 bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800 rounded-md">
                                    <p class="text-green-700 dark:text-green-300">{success}</p>
                                </div>
                            }

                            <SubNav
                                active_page={*active_page}
                                on_change={on_page_change}
                            />

                            <div class="overflow-y-auto">
                                {match *active_page {
                                    ConfigPage::Server => html! {
                                        <ServerConfigPage
                                            config={config.server.clone()}
                                            on_change={on_server_change}
                                        />
                                    },
                                    ConfigPage::Authentication => html! {
                                        <AuthConfigPage
                                            config={config.auth.clone()}
                                            on_change={on_auth_change}
                                        />
                                    },
                                    ConfigPage::Providers => html! {
                                        <ProvidersConfigPage
                                            providers={config.providers.clone()}
                                            on_change={on_providers_change}
                                        />
                                    },
                                    ConfigPage::Network => html! {
                                        <NetworkConfigPage
                                            tlsforward={config.tlsforward.clone()}
                                            letsencrypt={config.letsencrypt.clone()}
                                            on_tlsforward_change={on_tlsforward_change}
                                            on_letsencrypt_change={on_letsencrypt_change}
                                        />
                                    },
                                    ConfigPage::Inference => html! {
                                        <InferenceConfigPage
                                            config={config.local_inference.clone()}
                                            on_change={on_inference_change}
                                        />
                                    },
                                }}
                            </div>

                            <div class="mt-6 flex justify-end">
                                <button
                                    class="px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                                    onclick={on_save}
                                    disabled={*is_saving}
                                >
                                    if *is_saving {
                                        {"Saving..."}
                                    } else {
                                        {"Save Configuration"}
                                    }
                                </button>
                            </div>
                        </>
                    }
                </div>
            </div>
        </div>
    }
}
