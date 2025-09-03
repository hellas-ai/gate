use yew::prelude::*;

use super::super::shared::ConfigSection;
use super::super::types::ProviderConfig;
use super::ProviderGrid;

#[derive(Properties, PartialEq)]
pub struct ProvidersConfigSectionProps {
    pub providers: Vec<ProviderConfig>,
    pub on_change: Callback<Vec<ProviderConfig>>,
}

#[function_component(ProvidersConfigSection)]
pub fn providers_config_section(props: &ProvidersConfigSectionProps) -> Html {
    html! {
        <ConfigSection title="AI Providers">
            <ProviderGrid
                providers={props.providers.clone()}
                on_change={props.on_change.clone()}
            />
        </ConfigSection>
    }
}
