use super::super::{auth::AuthConfigSection, types::AuthConfig};
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct AuthConfigPageProps {
    pub config: AuthConfig,
    pub on_change: Callback<AuthConfig>,
}

#[function_component(AuthConfigPage)]
pub fn auth_config_page(props: &AuthConfigPageProps) -> Html {
    html! {
        <div class="p-6">
            <div class="mb-6">
                <h2 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
                    {"Authentication Settings"}
                </h2>
                <p class="mt-1 text-sm text-gray-600 dark:text-gray-400">
                    {"Configure authentication methods, WebAuthn, JWT, and registration policies"}
                </p>
            </div>

            <AuthConfigSection
                config={props.config.clone()}
                on_change={props.on_change.clone()}
            />
        </div>
    }
}
