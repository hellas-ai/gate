use super::super::{providers::ProvidersConfigSection, types::ProviderConfig};
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct ProvidersConfigPageProps {
    pub providers: Vec<ProviderConfig>,
    pub on_change: Callback<Vec<ProviderConfig>>,
}

#[function_component(ProvidersConfigPage)]
pub fn providers_config_page(props: &ProvidersConfigPageProps) -> Html {
    html! {
        <div class="p-6">
            <div class="mb-6">
                <h2 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
                    {"AI Provider Configuration"}
                </h2>
                <p class="mt-1 text-sm text-gray-600 dark:text-gray-400">
                    {"Manage AI provider connections, API keys, and model configurations"}
                </p>
            </div>

            <ProvidersConfigSection
                providers={props.providers.clone()}
                on_change={props.on_change.clone()}
            />
        </div>
    }
}
