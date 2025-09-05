use super::super::{server::ServerConfigSection, types::ServerConfig};
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct ServerConfigPageProps {
    pub config: ServerConfig,
    pub on_change: Callback<ServerConfig>,
}

#[function_component(ServerConfigPage)]
pub fn server_config_page(props: &ServerConfigPageProps) -> Html {
    html! {
        <div class="p-6">
            <div class="mb-6">
                <h2 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
                    {"Server Configuration"}
                </h2>
                <p class="mt-1 text-sm text-gray-600 dark:text-gray-400">
                    {"Configure the server's network settings and ports"}
                </p>
            </div>

            <ServerConfigSection
                config={props.config.clone()}
                on_change={props.on_change.clone()}
            />
        </div>
    }
}
