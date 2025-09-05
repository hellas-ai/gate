use super::super::types::ProviderConfig;
use super::provider_card::ProviderCard;
use super::provider_config_panel::ProviderConfigPanel;
use super::provider_registry::{get_all_providers, ProviderMetadata};
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct ProviderGridProps {
    pub providers: Vec<ProviderConfig>,
    pub on_change: Callback<Vec<ProviderConfig>>,
}

#[function_component(ProviderGrid)]
pub fn provider_grid(props: &ProviderGridProps) -> Html {
    let selected_provider = use_state(|| None::<&'static ProviderMetadata>);
    let editing_config = use_state(|| None::<ProviderConfig>);

    let all_providers = get_all_providers();

    // Check if a provider is configured
    let is_provider_configured =
        |provider_id: &str| -> bool { props.providers.iter().any(|p| p.provider == provider_id) };

    let on_provider_click = {
        let selected_provider = selected_provider.clone();
        let editing_config = editing_config.clone();
        let providers = props.providers.clone();

        Callback::from(move |provider_id: &'static str| {
            if let Some(provider_meta) = get_all_providers().iter().find(|p| p.id == provider_id) {
                selected_provider.set(Some(*provider_meta));

                // If provider is already configured, load its config for editing
                let config = providers
                    .iter()
                    .find(|p| p.provider == provider_id)
                    .cloned()
                    .unwrap_or_else(|| {
                        // Create new config with defaults
                        let name = if provider_id == "custom" {
                            format!("custom-{}", providers.len() + 1)
                        } else {
                            provider_id.to_string()
                        };
                        provider_meta.to_default_config(name)
                    });

                editing_config.set(Some(config));
            }
        })
    };

    let on_config_save = {
        let selected_provider = selected_provider.clone();
        let editing_config = editing_config.clone();
        let providers = props.providers.clone();
        let on_change = props.on_change.clone();

        Callback::from(move |new_config: ProviderConfig| {
            let mut updated_providers = providers.clone();

            // Find if this provider already exists
            if let Some(index) = updated_providers
                .iter()
                .position(|p| p.provider == new_config.provider && p.name == new_config.name)
            {
                // Update existing
                updated_providers[index] = new_config;
            } else if let Some(index) = updated_providers
                .iter()
                .position(|p| p.name == new_config.name)
            {
                // Update by name (in case provider type changed)
                updated_providers[index] = new_config;
            } else {
                // Add new
                updated_providers.push(new_config);
            }

            on_change.emit(updated_providers);
            selected_provider.set(None);
            editing_config.set(None);
        })
    };

    let on_config_delete = {
        let selected_provider = selected_provider.clone();
        let editing_config = editing_config.clone();
        let providers = props.providers.clone();
        let on_change = props.on_change.clone();

        Callback::from(move |config_name: String| {
            let updated_providers = providers
                .iter()
                .filter(|p| p.name != config_name)
                .cloned()
                .collect();

            on_change.emit(updated_providers);
            selected_provider.set(None);
            editing_config.set(None);
        })
    };

    let on_close = {
        let selected_provider = selected_provider.clone();
        let editing_config = editing_config.clone();

        Callback::from(move |_| {
            selected_provider.set(None);
            editing_config.set(None);
        })
    };

    html! {
        <div>
            // Configured providers section
            if !props.providers.is_empty() {
                <div class="mb-6">
                    <h3 class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">
                        {"Configured Providers"}
                    </h3>
                    <div class="space-y-2">
                        {props.providers.iter().map(|config| {
                            let on_edit = {
                                let config = config.clone();
                                let selected_provider = selected_provider.clone();
                                let editing_config = editing_config.clone();
                                Callback::from(move |_| {
                                    if let Some(provider_meta) = get_all_providers().iter().find(|p| p.id == config.provider) {
                                        selected_provider.set(Some(*provider_meta));
                                        editing_config.set(Some(config.clone()));
                                    }
                                })
                            };

                            let on_delete = {
                                let name = config.name.clone();
                                let on_change = props.on_change.clone();
                                let providers = props.providers.clone();
                                Callback::from(move |_| {
                                    let updated = providers.iter()
                                        .filter(|p| p.name != name)
                                        .cloned()
                                        .collect();
                                    on_change.emit(updated);
                                })
                            };

                            html! {
                                <div class="flex items-center justify-between p-3 bg-gray-50 dark:bg-gray-800 rounded-lg">
                                    <div class="flex items-center gap-3">
                                        <span class="text-sm font-medium text-gray-700 dark:text-gray-300">
                                            {&config.name}
                                        </span>
                                        <span class="text-xs text-gray-500 dark:text-gray-400">
                                            {format!("({})", config.provider)}
                                        </span>
                                    </div>
                                    <div class="flex items-center gap-2">
                                        <button
                                            class="text-blue-500 hover:text-blue-600 text-sm"
                                            onclick={on_edit}
                                            type="button"
                                        >
                                            {"Edit"}
                                        </button>
                                        <button
                                            class="text-red-500 hover:text-red-600 text-sm"
                                            onclick={on_delete}
                                            type="button"
                                        >
                                            {"Remove"}
                                        </button>
                                    </div>
                                </div>
                            }
                        }).collect::<Html>()}
                    </div>
                </div>
            }

            // Add new provider section
            <div>
                <h3 class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">
                    {"Add Provider"}
                </h3>
                <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 gap-4">
                    {all_providers.iter().map(|provider| {
                        html! {
                            <ProviderCard
                                provider={provider}
                                is_configured={is_provider_configured(provider.id)}
                                on_click={on_provider_click.clone()}
                            />
                        }
                    }).collect::<Html>()}
                </div>
            </div>

            // Configuration panel modal
            if let (Some(provider), Some(config)) = (*selected_provider, (*editing_config).clone()) {
                <ProviderConfigPanel
                    provider={provider}
                    config={config}
                    existing_names={props.providers.iter().map(|p| p.name.clone()).collect::<Vec<String>>()}
                    on_save={on_config_save}
                    on_delete={on_config_delete}
                    on_close={on_close}
                />
            }
        </div>
    }
}
