use yew::prelude::*;

use super::container::ServerConfig;
use super::shared::{ConfigField, ConfigInput, ConfigSection};

#[derive(Properties, PartialEq)]
pub struct ServerConfigSectionProps {
    pub config: ServerConfig,
    pub on_change: Callback<ServerConfig>,
}

#[function_component(ServerConfigSection)]
pub fn server_config_section(props: &ServerConfigSectionProps) -> Html {
    let config = props.config.clone();

    let on_host_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            let mut new_config = config.clone();
            new_config.host = value;
            on_change.emit(new_config);
        })
    };

    let on_port_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            if let Ok(port) = value.parse::<u16>() {
                let mut new_config = config.clone();
                new_config.port = port;
                on_change.emit(new_config);
            }
        })
    };

    let on_metrics_port_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            let mut new_config = config.clone();
            new_config.metrics_port = if value.is_empty() {
                None
            } else {
                value.parse::<u16>().ok()
            };
            on_change.emit(new_config);
        })
    };

    html! {
        <ConfigSection title="Server Configuration">
            <ConfigField
                label="Host"
                help_text="IP address to bind the server to"
            >
                <ConfigInput
                    value={props.config.host.clone()}
                    on_change={on_host_change}
                    placeholder="127.0.0.1"
                />
            </ConfigField>

            <ConfigField
                label="Port"
                help_text="Port number for the main server"
            >
                <ConfigInput
                    value={props.config.port.to_string()}
                    on_change={on_port_change}
                    input_type="number"
                    placeholder="31145"
                />
            </ConfigField>

            <ConfigField
                label="Metrics Port"
                help_text="Port for Prometheus metrics endpoint (optional)"
            >
                <ConfigInput
                    value={props.config.metrics_port.map_or(String::new(), |p| p.to_string())}
                    on_change={on_metrics_port_change}
                    input_type="number"
                    placeholder="9090"
                />
            </ConfigField>
        </ConfigSection>
    }
}
