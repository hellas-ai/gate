use yew::prelude::*;

use super::container::UpstreamConfig;
use super::shared::{ConfigField, ConfigInput, ConfigSection};

#[derive(Properties, PartialEq)]
pub struct UpstreamsConfigSectionProps {
    pub upstreams: Vec<UpstreamConfig>,
    pub on_change: Callback<Vec<UpstreamConfig>>,
}

#[function_component(UpstreamsConfigSection)]
pub fn upstreams_config_section(props: &UpstreamsConfigSectionProps) -> Html {
    let upstreams = props.upstreams.clone();

    let on_add_upstream = {
        let upstreams = upstreams.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |_| {
            let mut new_upstreams = upstreams.clone();
            new_upstreams.push(UpstreamConfig {
                name: format!("upstream-{}", new_upstreams.len() + 1),
                provider: "openai".to_string(),
                base_url: "https://api.openai.com".to_string(),
                api_key: None,
                timeout_seconds: 30,
                models: Vec::new(),
            });
            on_change.emit(new_upstreams);
        })
    };

    html! {
        <ConfigSection title="Upstream Providers">
            <div class="space-y-4">
                {for upstreams.iter().enumerate().map(|(index, upstream)| {
                    let upstream = upstream.clone();
                    let on_name_change = {
                        let upstreams = props.upstreams.clone();
                        let on_change = props.on_change.clone();
                        Callback::from(move |value: String| {
                            let mut new_upstreams = upstreams.clone();
                            new_upstreams[index].name = value;
                            on_change.emit(new_upstreams);
                        })
                    };

                    let on_provider_change = {
                        let upstreams = props.upstreams.clone();
                        let on_change = props.on_change.clone();
                        Callback::from(move |e: Event| {
                            let input: web_sys::HtmlSelectElement = e.target_unchecked_into();
                            let mut new_upstreams = upstreams.clone();
                            new_upstreams[index].provider = input.value();
                            on_change.emit(new_upstreams);
                        })
                    };

                    let on_base_url_change = {
                        let upstreams = props.upstreams.clone();
                        let on_change = props.on_change.clone();
                        Callback::from(move |value: String| {
                            let mut new_upstreams = upstreams.clone();
                            new_upstreams[index].base_url = value;
                            on_change.emit(new_upstreams);
                        })
                    };

                    let on_api_key_change = {
                        let upstreams = props.upstreams.clone();
                        let on_change = props.on_change.clone();
                        Callback::from(move |value: String| {
                            let mut new_upstreams = upstreams.clone();
                            new_upstreams[index].api_key = if value.is_empty() {
                                None
                            } else {
                                Some(value)
                            };
                            on_change.emit(new_upstreams);
                        })
                    };

                    let on_timeout_change = {
                        let upstreams = props.upstreams.clone();
                        let on_change = props.on_change.clone();
                        Callback::from(move |value: String| {
                            if let Ok(timeout) = value.parse::<u64>() {
                                let mut new_upstreams = upstreams.clone();
                                new_upstreams[index].timeout_seconds = timeout;
                                on_change.emit(new_upstreams);
                            }
                        })
                    };

                    let on_remove = {
                        let upstreams = props.upstreams.clone();
                        let on_change = props.on_change.clone();
                        Callback::from(move |_| {
                            let mut new_upstreams = upstreams.clone();
                            new_upstreams.remove(index);
                            on_change.emit(new_upstreams);
                        })
                    };

                    html! {
                        <div class="p-4 border border-gray-200 dark:border-gray-700 rounded-lg">
                            <div class="flex justify-between items-start mb-3">
                                <h4 class="text-sm font-medium text-gray-700 dark:text-gray-300">
                                    {format!("Upstream #{}", index + 1)}
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
                                    help_text="Unique identifier for this upstream"
                                >
                                    <ConfigInput
                                        value={upstream.name.clone()}
                                        on_change={on_name_change}
                                        placeholder="my-upstream"
                                    />
                                </ConfigField>

                                <ConfigField
                                    label="Provider"
                                    help_text="Type of AI provider"
                                >
                                    <select
                                        class="w-full px-2.5 py-1.5 text-sm border border-gray-300 dark:border-gray-600 rounded-md shadow-sm
                                               focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500
                                               bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100"
                                        value={upstream.provider.clone()}
                                        onchange={on_provider_change}
                                    >
                                        <option value="openai">{"OpenAI"}</option>
                                        <option value="anthropic">{"Anthropic"}</option>
                                        <option value="cohere">{"Cohere"}</option>
                                        <option value="groq">{"Groq"}</option>
                                        <option value="mistral">{"Mistral"}</option>
                                        <option value="perplexity">{"Perplexity"}</option>
                                        <option value="together">{"Together"}</option>
                                        <option value="deepinfra">{"DeepInfra"}</option>
                                        <option value="openrouter">{"OpenRouter"}</option>
                                        <option value="custom">{"Custom"}</option>
                                    </select>
                                </ConfigField>

                                <ConfigField
                                    label="Base URL"
                                    help_text="API endpoint URL"
                                >
                                    <ConfigInput
                                        value={upstream.base_url.clone()}
                                        on_change={on_base_url_change}
                                        placeholder="https://api.openai.com"
                                    />
                                </ConfigField>

                                <ConfigField
                                    label="API Key"
                                    help_text="Authentication key for the provider"
                                >
                                    <ConfigInput
                                        value={upstream.api_key.clone().unwrap_or_default()}
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
                                        value={upstream.timeout_seconds.to_string()}
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
                    onclick={on_add_upstream}
                    type="button"
                >
                    {"+ Add Upstream Provider"}
                </button>
            </div>
        </ConfigSection>
    }
}
