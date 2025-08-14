use yew::prelude::*;

use super::container::AuthConfig;
use super::shared::{ConfigField, ConfigInput, ConfigSection, ConfigToggle};

#[derive(Properties, PartialEq)]
pub struct AuthConfigSectionProps {
    pub config: AuthConfig,
    pub on_change: Callback<AuthConfig>,
}

#[function_component(AuthConfigSection)]
pub fn auth_config_section(props: &AuthConfigSectionProps) -> Html {
    let config = props.config.clone();

    let on_webauthn_enabled_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: bool| {
            let mut new_config = config.clone();
            new_config.webauthn.enabled = value;
            on_change.emit(new_config);
        })
    };

    let on_rp_id_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            let mut new_config = config.clone();
            new_config.webauthn.rp_id = value;
            on_change.emit(new_config);
        })
    };

    let on_rp_name_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            let mut new_config = config.clone();
            new_config.webauthn.rp_name = value;
            on_change.emit(new_config);
        })
    };

    let on_rp_origin_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            let mut new_config = config.clone();
            new_config.webauthn.rp_origin = value;
            on_change.emit(new_config);
        })
    };

    let on_allowed_origins_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            let mut new_config = config.clone();
            new_config.webauthn.allowed_origins = value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            on_change.emit(new_config);
        })
    };

    let on_jwt_issuer_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            let mut new_config = config.clone();
            new_config.jwt.issuer = value;
            on_change.emit(new_config);
        })
    };

    let on_jwt_secret_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            let mut new_config = config.clone();
            new_config.jwt.secret = value;
            on_change.emit(new_config);
        })
    };

    let on_jwt_expiration_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            if let Ok(hours) = value.parse::<u64>() {
                let mut new_config = config.clone();
                new_config.jwt.expiration_hours = hours;
                on_change.emit(new_config);
            }
        })
    };

    let on_open_registration_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: bool| {
            let mut new_config = config.clone();
            new_config.registration.allow_open_registration = value;
            on_change.emit(new_config);
        })
    };

    html! {
        <ConfigSection title="Authentication Configuration">
            <div class="space-y-6">
                <ConfigSection
                    title="WebAuthn Settings"
                    enabled={props.config.webauthn.enabled}
                    on_toggle={on_webauthn_enabled_change}
                >
                    <ConfigField
                        label="Relying Party ID"
                        help_text="Usually your domain name (e.g., example.com)"
                    >
                        <ConfigInput
                            value={props.config.webauthn.rp_id.clone()}
                            on_change={on_rp_id_change}
                            placeholder="localhost"
                        />
                    </ConfigField>

                    <ConfigField
                        label="Relying Party Name"
                        help_text="Display name for your application"
                    >
                        <ConfigInput
                            value={props.config.webauthn.rp_name.clone()}
                            on_change={on_rp_name_change}
                            placeholder="Gate Self-Hosted"
                        />
                    </ConfigField>

                    <ConfigField
                        label="Relying Party Origin"
                        help_text="Full URL of your application"
                    >
                        <ConfigInput
                            value={props.config.webauthn.rp_origin.clone()}
                            on_change={on_rp_origin_change}
                            placeholder="http://localhost:31145"
                        />
                    </ConfigField>

                    <ConfigField
                        label="Additional Allowed Origins"
                        help_text="Comma-separated list of additional allowed origins"
                    >
                        <ConfigInput
                            value={props.config.webauthn.allowed_origins.join(", ")}
                            on_change={on_allowed_origins_change}
                            placeholder="https://app.example.com, https://beta.example.com"
                        />
                    </ConfigField>
                </ConfigSection>

                <div>
                    <h4 class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">{"JWT Settings"}</h4>
                    <div class="pl-4 space-y-3">
                        <ConfigField
                            label="JWT Issuer"
                            help_text="Issuer identifier for JWT tokens"
                        >
                            <ConfigInput
                                value={props.config.jwt.issuer.clone()}
                                on_change={on_jwt_issuer_change}
                                placeholder="gate-daemon"
                            />
                        </ConfigField>

                        <ConfigField
                            label="JWT Secret"
                            help_text="Secret key for signing JWT tokens (keep this secure!)"
                        >
                            <ConfigInput
                                value={props.config.jwt.secret.clone()}
                                on_change={on_jwt_secret_change}
                                input_type="password"
                                placeholder="Your secret key"
                            />
                        </ConfigField>

                        <ConfigField
                            label="Token Expiration (hours)"
                            help_text="How long JWT tokens remain valid"
                        >
                            <ConfigInput
                                value={props.config.jwt.expiration_hours.to_string()}
                                on_change={on_jwt_expiration_change}
                                input_type="number"
                                placeholder="24"
                            />
                        </ConfigField>
                    </div>
                </div>

                <div>
                    <h4 class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">{"Registration Settings"}</h4>
                    <div class="pl-4">
                        <ConfigToggle
                            label="Allow Open Registration"
                            checked={props.config.registration.allow_open_registration}
                            on_change={on_open_registration_change}
                            help_text="Allow new users to register without an invitation"
                        />
                    </div>
                </div>
            </div>
        </ConfigSection>
    }
}
