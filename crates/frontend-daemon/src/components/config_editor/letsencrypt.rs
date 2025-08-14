use yew::prelude::*;

use super::container::LetsEncryptConfig;
use super::shared::{ConfigField, ConfigInput, ConfigSection, ConfigToggle};

#[derive(Properties, PartialEq)]
pub struct LetsEncryptConfigSectionProps {
    pub config: LetsEncryptConfig,
    pub on_change: Callback<LetsEncryptConfig>,
}

#[function_component(LetsEncryptConfigSection)]
pub fn letsencrypt_config_section(props: &LetsEncryptConfigSectionProps) -> Html {
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

    let on_email_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            let mut new_config = config.clone();
            new_config.email = if value.is_empty() { None } else { Some(value) };
            on_change.emit(new_config);
        })
    };

    let on_staging_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: bool| {
            let mut new_config = config.clone();
            new_config.staging = value;
            on_change.emit(new_config);
        })
    };

    let on_domains_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            let mut new_config = config.clone();
            new_config.domains = value
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            on_change.emit(new_config);
        })
    };

    let on_auto_renew_days_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            if let Ok(days) = value.parse::<u32>() {
                let mut new_config = config.clone();
                new_config.auto_renew_days = days;
                on_change.emit(new_config);
            }
        })
    };

    html! {
        <ConfigSection
            title="Let's Encrypt Configuration"
            enabled={props.config.enabled}
            on_toggle={on_enabled_change}
        >
            <ConfigField
                label="Email Address"
                help_text="Email for Let's Encrypt account and notifications"
            >
                <ConfigInput
                    value={props.config.email.clone().unwrap_or_default()}
                    on_change={on_email_change}
                    input_type="email"
                    placeholder="admin@example.com"
                />
            </ConfigField>

            <ConfigToggle
                label="Use Staging Environment"
                checked={props.config.staging}
                on_change={on_staging_change}
                help_text="Use Let's Encrypt staging servers for testing (certificates won't be trusted)"
            />

            <ConfigField
                label="Domains"
                help_text="Domain names to request certificates for (one per line)"
            >
                <textarea
                    class="w-full px-2.5 py-1.5 text-sm border border-gray-300 dark:border-gray-600 rounded-md shadow-sm
                           focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500
                           bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100"
                    rows="2"
                    value={props.config.domains.join("\n")}
                    oninput={Callback::from(move |e: InputEvent| {
                        let input: web_sys::HtmlTextAreaElement = e.target_unchecked_into();
                        on_domains_change.emit(input.value());
                    })}
                    placeholder={"example.com\nwww.example.com\napi.example.com"}
                />
            </ConfigField>

            <ConfigField
                label="Auto-Renew Days Before Expiry"
                help_text="Number of days before certificate expiry to attempt renewal"
            >
                <ConfigInput
                    value={props.config.auto_renew_days.to_string()}
                    on_change={on_auto_renew_days_change}
                    input_type="number"
                    placeholder="30"
                />
            </ConfigField>
        </ConfigSection>
    }
}
