use yew::prelude::*;

use super::container::ProviderConfig;
use super::shared::{ConfigField, ConfigInput, ConfigSection};

#[derive(Properties, PartialEq)]
pub struct ProvidersConfigSectionProps {
    pub providers: Vec<ProviderConfig>,
    pub on_change: Callback<Vec<ProviderConfig>>,
}

#[function_component(ProvidersConfigSection)]
pub fn providers_config_section(props: &ProvidersConfigSectionProps) -> Html {
    let providers = props.providers.clone();

    let on_add_provider = {
        let providers = providers.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |_| {
            let mut new_providers = providers.clone();
            new_providers.push(ProviderConfig {
                name: format!("provider-{}", new_providers.len() + 1),
                provider: "openai".to_string(),
                base_url: "https://api.openai.com".to_string(),
                api_key: None,
                timeout_seconds: 30,
                models: Vec::new(),
            });
            on_change.emit(new_providers);
        })
    };

    html! {
        <ConfigSection title="Providers">
            <div class="space-y-4">
                {for providers.iter().enumerate().map(|(index, provider)| {
                    let provider = provider.clone();
                    let on_name_change = {
                        let providers = props.providers.clone();
                        let on_change = props.on_change.clone();
                        Callback::from(move |value: String| {
                            let mut new_providers = providers.clone();
                            new_providers[index].name = value;
                            on_change.emit(new_providers);
                        })
                    };

                    let on_provider_change = {
                        let providers = props.providers.clone();
                        let on_change = props.on_change.clone();
                        Callback::from(move |e: Event| {
                            let input: web_sys::HtmlSelectElement = e.target_unchecked_into();
                            let mut new_providers = providers.clone();
                            let provider_type = input.value();
                            new_providers[index].provider = provider_type.clone();

                            // Update base_url based on provider selection
                            new_providers[index].base_url = match provider_type.as_str() {
                                "openai" => "https://api.openai.com".to_string(),
                                "anthropic" => "https://api.anthropic.com".to_string(),
                                _ => new_providers[index].base_url.clone(),
                            };

                            on_change.emit(new_providers);
                        })
                    };

                    let on_base_url_change = {
                        let providers = props.providers.clone();
                        let on_change = props.on_change.clone();
                        Callback::from(move |value: String| {
                            let mut new_providers = providers.clone();
                            new_providers[index].base_url = value;
                            on_change.emit(new_providers);
                        })
                    };

                    let on_api_key_change = {
                        let providers = props.providers.clone();
                        let on_change = props.on_change.clone();
                        Callback::from(move |value: String| {
                            let mut new_providers = providers.clone();
                            new_providers[index].api_key = if value.is_empty() {
                                None
                            } else {
                                Some(value)
                            };
                            on_change.emit(new_providers);
                        })
                    };

                    let on_timeout_change = {
                        let providers = props.providers.clone();
                        let on_change = props.on_change.clone();
                        Callback::from(move |value: String| {
                            if let Ok(timeout) = value.parse::<u64>() {
                                let mut new_providers = providers.clone();
                                new_providers[index].timeout_seconds = timeout;
                                on_change.emit(new_providers);
                            }
                        })
                    };

                    let on_remove = {
                        let providers = props.providers.clone();
                        let on_change = props.on_change.clone();
                        Callback::from(move |_| {
                            let mut new_providers = providers.clone();
                            new_providers.remove(index);
                            on_change.emit(new_providers);
                        })
                    };

                    html! {
                        <div class="p-4 border border-gray-200 dark:border-gray-700 rounded-lg">
                            <div class="flex justify-between items-start mb-3">
                                <h4 class="text-sm font-medium text-gray-700 dark:text-gray-300">
                                    {format!("Provider #{}", index + 1)}
                                </h4>
                                <button
                                    class="text-red-500 hover:text-red-600 text-sm"
                                    onclick={on_remove}
                                    type="button"
                                >
                                    {"Remove"}
                                </button>
                            </div>

                            <div class="space-y-3">
                                <ConfigField
                                    label="Name"
                                    help_text="Unique identifier for this provider"
                                >
                                    <ConfigInput
                                        value={provider.name.clone()}
                                        on_change={on_name_change}
                                        placeholder="my-provider"
                                    />
                                </ConfigField>

                                <ConfigField
                                    label="Provider"
                                    help_text="Select OpenAI, Anthropic, or Custom for other providers (Groq, Mistral, etc.)"
                                >
                                    <select
                                        class="w-full px-2.5 py-1.5 text-sm border border-gray-300 dark:border-gray-600 rounded-md shadow-sm
                                               focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500
                                               bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100"
                                        value={provider.provider.clone()}
                                        onchange={on_provider_change}
                                    >
                                        <option value="openai">{"OpenAI"}</option>
                                        <option value="anthropic">{"Anthropic"}</option>
                                        <option value="custom">{"Custom"}</option>
                                    </select>
                                </ConfigField>

                                <ConfigField
                                    label="Base URL"
                                    help_text="API endpoint URL"
                                >
                                    <ConfigInput
                                        value={provider.base_url.clone()}
                                        on_change={on_base_url_change}
                                        placeholder="https://api.example.com"
                                    />
                                </ConfigField>

                                <ConfigField
                                    label="API Key"
                                    help_text="Authentication key for the provider"
                                >
                                    <ConfigInput
                                        value={provider.api_key.clone().unwrap_or_default()}
                                        on_change={on_api_key_change}
                                        input_type="password"
                                        placeholder="sk-..."
                                    />
                                </ConfigField>

                                <ConfigField
                                    label="Timeout (seconds)"
                                    help_text="Request timeout in seconds"
                                >
                                    <ConfigInput
                                        value={provider.timeout_seconds.to_string()}
                                        on_change={on_timeout_change}
                                        input_type="number"
                                        placeholder="30"
                                    />
                                </ConfigField>
                            </div>
                        </div>
                    }
                })}

                <button
                    class="w-full py-2 px-4 border-2 border-dashed border-gray-300 dark:border-gray-600 rounded-lg
                           text-gray-600 dark:text-gray-400 hover:border-gray-400 dark:hover:border-gray-500
                           hover:text-gray-700 dark:hover:text-gray-300 transition-colors"
                    onclick={on_add_provider}
                    type="button"
                >
                    {"+ Add Provider"}
                </button>
            </div>
        </ConfigSection>
    }
}
