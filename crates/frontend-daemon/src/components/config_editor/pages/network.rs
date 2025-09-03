use super::super::{
    letsencrypt::LetsEncryptConfigSection,
    tlsforward::TlsForwardConfigSection,
    types::{LetsEncryptConfig, TlsForwardConfig},
};
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct NetworkConfigPageProps {
    pub tlsforward: TlsForwardConfig,
    pub letsencrypt: LetsEncryptConfig,
    pub on_tlsforward_change: Callback<TlsForwardConfig>,
    pub on_letsencrypt_change: Callback<LetsEncryptConfig>,
}

#[function_component(NetworkConfigPage)]
pub fn network_config_page(props: &NetworkConfigPageProps) -> Html {
    html! {
        <div class="p-6">
            <div class="mb-6">
                <h2 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
                    {"Network & Security"}
                </h2>
                <p class="mt-1 text-sm text-gray-600 dark:text-gray-400">
                    {"Configure TLS forwarding, certificates, and network security settings"}
                </p>
            </div>

            <div class="space-y-6">
                <div>
                    <h3 class="text-md font-medium text-gray-800 dark:text-gray-200 mb-3">
                        {"TLS Forwarding"}
                    </h3>
                    <TlsForwardConfigSection
                        config={props.tlsforward.clone()}
                        on_change={props.on_tlsforward_change.clone()}
                    />
                </div>

                <div>
                    <h3 class="text-md font-medium text-gray-800 dark:text-gray-200 mb-3">
                        {"Let's Encrypt Certificates"}
                    </h3>
                    <LetsEncryptConfigSection
                        config={props.letsencrypt.clone()}
                        on_change={props.on_letsencrypt_change.clone()}
                    />
                </div>
            </div>
        </div>
    }
}
