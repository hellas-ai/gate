use yew::prelude::*;

use super::container::TlsForwardConfig;
use super::shared::{ConfigField, ConfigInput, ConfigSection};

#[derive(Properties, PartialEq)]
pub struct TlsForwardConfigSectionProps {
    pub config: TlsForwardConfig,
    pub on_change: Callback<TlsForwardConfig>,
}

#[function_component(TlsForwardConfigSection)]
pub fn tlsforward_config_section(props: &TlsForwardConfigSectionProps) -> Html {
    let config = props.config.clone();

    let on_enabled_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: bool| {
            let mut new_config = config.clone();
            new_config.enabled = value;
            on_change.emit(new_config);
        })
    };

    let on_addresses_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            let mut new_config = config.clone();
            new_config.tlsforward_addresses = value
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            on_change.emit(new_config);
        })
    };

    let on_max_connections_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            if let Ok(max_conn) = value.parse::<usize>() {
                let mut new_config = config.clone();
                new_config.max_connections = max_conn;
                on_change.emit(new_config);
            }
        })
    };

    let on_secret_key_path_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            let mut new_config = config.clone();
            new_config.secret_key_path = if value.is_empty() { None } else { Some(value) };
            on_change.emit(new_config);
        })
    };

    let on_heartbeat_interval_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            if let Ok(interval) = value.parse::<u64>() {
                let mut new_config = config.clone();
                new_config.heartbeat_interval = interval;
                on_change.emit(new_config);
            }
        })
    };

    html! {
        <ConfigSection
            title="TLS Forward Configuration"
            enabled={props.config.enabled}
            on_toggle={on_enabled_change}
        >
            <ConfigField
                label="TLS Forward Server Addresses"
                help_text="Node addresses in format: node_id@host:port (one per line)"
            >
                <textarea
                    class="w-full px-2.5 py-1.5 text-sm border border-gray-300 dark:border-gray-600 rounded-md shadow-sm
                           focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500
                           bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100 font-mono"
                    rows="2"
                    value={props.config.tlsforward_addresses.join("\n")}
                    oninput={Callback::from(move |e: InputEvent| {
                        let input: web_sys::HtmlTextAreaElement = e.target_unchecked_into();
                        on_addresses_change.emit(input.value());
                    })}
                    placeholder="3dbefb2e3d56c7e32586d9a82167a8a5151f3e0f4b40b7c3d145b9060dde2f14@213.239.212.173:31145"
                />
            </ConfigField>

            <ConfigField
                label="Maximum Connections"
                help_text="Maximum number of concurrent TLS connections"
            >
                <ConfigInput
                    value={props.config.max_connections.to_string()}
                    on_change={on_max_connections_change}
                    input_type="number"
                    placeholder="1000"
                />
            </ConfigField>

            <ConfigField
                label="Secret Key Path"
                help_text="Path to store the secret key for persistent node ID (optional)"
            >
                <ConfigInput
                    value={props.config.secret_key_path.clone().unwrap_or_default()}
                    on_change={on_secret_key_path_change}
                    placeholder="/path/to/secret.key"
                />
            </ConfigField>

            <ConfigField
                label="Heartbeat Interval (seconds)"
                help_text="Interval for sending keepalive messages"
            >
                <ConfigInput
                    value={props.config.heartbeat_interval.to_string()}
                    on_change={on_heartbeat_interval_change}
                    input_type="number"
                    placeholder="30"
                />
            </ConfigField>
        </ConfigSection>
    }
}
