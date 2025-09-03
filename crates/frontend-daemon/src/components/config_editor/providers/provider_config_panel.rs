use super::super::{
    shared::{ConfigField, ConfigInput},
    types::ProviderConfig,
};
use super::provider_registry::ProviderMetadata;
use web_sys::HtmlInputElement;
use yew::prelude::*;

#[derive(Properties)]
pub struct ProviderConfigPanelProps {
    pub provider: &'static ProviderMetadata,
    pub config: ProviderConfig,
    pub existing_names: Vec<String>,
    pub on_save: Callback<ProviderConfig>,
    pub on_delete: Callback<String>,
    pub on_close: Callback<()>,
}

impl PartialEq for ProviderConfigPanelProps {
    fn eq(&self, other: &Self) -> bool {
        self.provider.id == other.provider.id
            && self.config == other.config
            && self.existing_names == other.existing_names
    }
}

#[function_component(ProviderConfigPanel)]
pub fn provider_config_panel(props: &ProviderConfigPanelProps) -> Html {
    let config = use_state(|| props.config.clone());
    let is_editing = props.existing_names.contains(&props.config.name);

    let on_name_change = {
        let config = config.clone();
        Callback::from(move |value: String| {
            let mut new_config = (*config).clone();
            new_config.name = value;
            config.set(new_config);
        })
    };

    let on_base_url_change = {
        let config = config.clone();
        Callback::from(move |value: String| {
            let mut new_config = (*config).clone();
            new_config.base_url = value;
            config.set(new_config);
        })
    };

    let on_api_key_change = {
        let config = config.clone();
        Callback::from(move |value: String| {
            let mut new_config = (*config).clone();
            new_config.api_key = if value.is_empty() { None } else { Some(value) };
            config.set(new_config);
        })
    };

    let on_timeout_change = {
        let config = config.clone();
        Callback::from(move |value: String| {
            if let Ok(timeout) = value.parse::<u64>() {
                let mut new_config = (*config).clone();
                new_config.timeout_seconds = timeout;
                config.set(new_config);
            }
        })
    };

    let on_models_change = {
        let config = config.clone();
        Callback::from(move |e: InputEvent| {
            let input: HtmlInputElement = e.target_unchecked_into();
            let value = input.value();
            let mut new_config = (*config).clone();
            new_config.models = value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            config.set(new_config);
        })
    };

    let on_save = {
        let config = config.clone();
        let on_save = props.on_save.clone();
        let existing_names = props.existing_names.clone();
        let original_name = props.config.name.clone();

        Callback::from(move |e: MouseEvent| {
            e.prevent_default();

            let new_config = (*config).clone();

            // Validate name is not empty
            if new_config.name.is_empty() {
                return;
            }

            // Check for duplicate names (except when editing the same config)
            if new_config.name != original_name && existing_names.contains(&new_config.name) {
                return;
            }

            on_save.emit(new_config);
        })
    };

    let on_delete = {
        let config_name = props.config.name.clone();
        let on_delete = props.on_delete.clone();

        Callback::from(move |e: MouseEvent| {
            e.prevent_default();
            on_delete.emit(config_name.clone());
        })
    };

    let on_close = {
        let on_close = props.on_close.clone();
        Callback::from(move |_| {
            on_close.emit(());
        })
    };

    let on_backdrop_click = {
        let on_close = props.on_close.clone();
        Callback::from(move |e: MouseEvent| {
            e.prevent_default();
            on_close.emit(());
        })
    };

    let on_panel_click = Callback::from(|e: MouseEvent| {
        e.stop_propagation();
    });

    html! {
        <div
            class="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center p-4 z-50"
            onclick={on_backdrop_click}
        >
            <div
                class="bg-white dark:bg-gray-800 rounded-lg shadow-xl max-w-2xl w-full max-h-[90vh] overflow-y-auto"
                onclick={on_panel_click}
            >
                // Header
                <div class="border-b border-gray-200 dark:border-gray-700 px-6 py-4">
                    <div class="flex items-center justify-between">
                        <div class="flex items-center gap-3">
                            <img
                                src={props.provider.icon_path}
                                alt={props.provider.display_name}
                                class="w-8 h-8 object-contain"
                            />
                            <h2 class="text-xl font-semibold text-gray-800 dark:text-gray-200">
                                {format!("Configure {}", props.provider.display_name)}
                            </h2>
                        </div>
                        <button
                            class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
                            onclick={on_close.clone()}
                            type="button"
                        >
                            <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path>
                            </svg>
                        </button>
                    </div>
                </div>

                // Content
                <div class="p-6 space-y-4">
                    <ConfigField
                        label="Configuration Name"
                        help_text="Unique identifier for this provider configuration"
                    >
                        <ConfigInput
                            value={config.name.clone()}
                            on_change={on_name_change}
                            placeholder="my-provider"
                        />
                    </ConfigField>

                    <ConfigField
                        label="Base URL"
                        help_text="API endpoint URL"
                    >
                        <ConfigInput
                            value={config.base_url.clone()}
                            on_change={on_base_url_change}
                            placeholder={props.provider.default_base_url}
                        />
                    </ConfigField>

                    if props.provider.requires_api_key {
                        <ConfigField
                            label="API Key"
                            help_text="Authentication key for the provider"
                        >
                            <ConfigInput
                                value={config.api_key.clone().unwrap_or_default()}
                                on_change={on_api_key_change}
                                input_type="password"
                                placeholder={props.provider.placeholder_api_key}
                            />
                        </ConfigField>
                    }

                    <ConfigField
                        label="Timeout (seconds)"
                        help_text="Request timeout in seconds"
                    >
                        <ConfigInput
                            value={config.timeout_seconds.to_string()}
                            on_change={on_timeout_change}
                            input_type="number"
                            placeholder="30"
                        />
                    </ConfigField>

                    <ConfigField
                        label="Supported Models"
                        help_text="Comma-separated list of model identifiers"
                    >
                        <input
                            type="text"
                            value={config.models.join(", ")}
                            oninput={on_models_change}
                            placeholder={if !props.provider.supported_models.is_empty() {
                                props.provider.supported_models.join(", ")
                            } else {
                                "gpt-4, gpt-3.5-turbo".to_string()
                            }}
                            class="w-full px-2.5 py-1.5 text-sm border border-gray-300 dark:border-gray-600 rounded-md shadow-sm
                                   focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500
                                   bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100"
                        />
                    </ConfigField>

                    // Default headers info (read-only)
                    if !props.provider.default_headers.is_empty() {
                        <div class="p-3 bg-blue-50 dark:bg-blue-900/20 rounded-md">
                            <p class="text-sm text-blue-700 dark:text-blue-300">
                                {"Default headers will be automatically included:"}
                            </p>
                            <ul class="mt-1 text-xs text-blue-600 dark:text-blue-400">
                                {props.provider.default_headers.iter().map(|(key, value)| {
                                    html! {
                                        <li>{format!("{}: {}", key, value)}</li>
                                    }
                                }).collect::<Html>()}
                            </ul>
                        </div>
                    }
                </div>

                // Footer
                <div class="border-t border-gray-200 dark:border-gray-700 px-6 py-4">
                    <div class="flex justify-between">
                        <div>
                            if is_editing {
                                <button
                                    class="px-4 py-2 text-red-600 hover:text-red-700 dark:text-red-400 dark:hover:text-red-300"
                                    onclick={on_delete}
                                    type="button"
                                >
                                    {"Delete"}
                                </button>
                            }
                        </div>
                        <div class="flex gap-3">
                            <button
                                class="px-4 py-2 text-gray-600 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-300"
                                onclick={on_close}
                                type="button"
                            >
                                {"Cancel"}
                            </button>
                            <button
                                class="px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white rounded-md transition-colors"
                                onclick={on_save}
                                type="button"
                            >
                                {if is_editing { "Update" } else { "Add Provider" }}
                            </button>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
}
