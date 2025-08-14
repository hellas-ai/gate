use crate::services::ConfigApiService;
use gloo::timers::callback::Timeout;
use yew::prelude::*;

#[function_component(ConfigEditor)]
pub fn config_editor() -> Html {
    let config_service = use_memo((), |_| ConfigApiService::new());
    let config_json = use_state(|| String::from("{}"));
    let is_loading = use_state(|| false);
    let error_message = use_state(|| None::<String>);
    let success_message = use_state(|| None::<String>);

    // Load config on mount
    {
        let config_service = config_service.clone();
        let config_json = config_json.clone();
        let is_loading = is_loading.clone();
        let error_message = error_message.clone();

        use_effect_with((), move |_| {
            is_loading.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                match config_service.get_config().await {
                    Ok(config) => match serde_json::to_string_pretty(&config) {
                        Ok(json) => config_json.set(json),
                        Err(e) => error_message.set(Some(format!("Failed to format config: {e}"))),
                    },
                    Err(e) => {
                        // Auth errors are handled automatically by the client wrapper
                        error_message.set(Some(format!("Failed to load config: {e}")));
                    }
                }
                is_loading.set(false);
            });
        });
    }

    let on_config_change = {
        let config_json = config_json.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlTextAreaElement = e.target_unchecked_into();
            config_json.set(input.value());
        })
    };

    let on_save = {
        let config_service = config_service.clone();
        let config_json = config_json.clone();
        let is_loading = is_loading.clone();
        let error_message = error_message.clone();
        let success_message = success_message.clone();

        Callback::from(move |_| {
            let json_str = (*config_json).clone();

            // Validate JSON
            match serde_json::from_str::<serde_json::Value>(&json_str) {
                Ok(config) => {
                    is_loading.set(true);
                    error_message.set(None);

                    let config_service = config_service.clone();
                    let is_loading = is_loading.clone();
                    let error_message = error_message.clone();
                    let success_message = success_message.clone();

                    wasm_bindgen_futures::spawn_local(async move {
                        match config_service.update_config(config).await {
                            Ok(_) => {
                                success_message
                                    .set(Some("Configuration saved successfully!".to_string()));
                                // Clear success message after 3 seconds
                                let success_message = success_message.clone();
                                Timeout::new(3000, move || {
                                    success_message.set(None);
                                })
                                .forget();
                            }
                            Err(e) => {
                                error_message.set(Some(format!("Failed to save config: {e}")))
                            }
                        }
                        is_loading.set(false);
                    });
                }
                Err(e) => {
                    error_message.set(Some(format!("Invalid JSON: {e}")));
                }
            }
        })
    };

    let on_format = {
        let config_json = config_json.clone();
        let error_message = error_message.clone();

        Callback::from(move |_| {
            let json_str = (*config_json).clone();
            match serde_json::from_str::<serde_json::Value>(&json_str) {
                Ok(config) => match serde_json::to_string_pretty(&config) {
                    Ok(formatted) => {
                        config_json.set(formatted);
                        error_message.set(None);
                    }
                    Err(e) => error_message.set(Some(format!("Failed to format: {e}"))),
                },
                Err(e) => {
                    error_message.set(Some(format!("Invalid JSON: {e}")));
                }
            }
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
                        {"Edit the Gate configuration in JSON format"}
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

                            <div class="mb-4">
                                <textarea
                                    class="w-full h-96 px-4 py-3 font-mono text-sm bg-gray-50 dark:bg-gray-900 border border-gray-300 dark:border-gray-600 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500"
                                    value={(*config_json).clone()}
                                    oninput={on_config_change}
                                    placeholder="Loading configuration..."
                                />
                            </div>

                            <div class="flex gap-3">
                                <button
                                    class="px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                                    onclick={on_save}
                                    disabled={*is_loading}
                                >
                                    {"Save Configuration"}
                                </button>
                                <button
                                    class="px-4 py-2 bg-gray-200 hover:bg-gray-300 dark:bg-gray-700 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-md transition-colors"
                                    onclick={on_format}
                                >
                                    {"Format JSON"}
                                </button>
                            </div>
                        </>
                    }
                </div>
            </div>
        </div>
    }
}
