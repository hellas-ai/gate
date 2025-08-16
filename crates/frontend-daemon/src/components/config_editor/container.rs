use crate::services::ConfigApiService;
use gloo::timers::callback::Timeout;
use serde::{Deserialize, Serialize};
use yew::prelude::*;

use super::{
    auth::AuthConfigSection, inference::InferenceConfigSection,
    letsencrypt::LetsEncryptConfigSection, server::ServerConfigSection,
    tlsforward::TlsForwardConfigSection, upstreams::UpstreamsConfigSection,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GateConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub upstreams: Vec<UpstreamConfig>,
    #[serde(default)]
    pub tlsforward: TlsForwardConfig,
    #[serde(default)]
    pub letsencrypt: LetsEncryptConfig,
    #[serde(default)]
    pub local_inference: Option<LocalInferenceConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub metrics_port: Option<u16>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            metrics_port: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AuthConfig {
    #[serde(default)]
    pub webauthn: WebAuthnConfig,
    #[serde(default)]
    pub jwt: JwtConfig,
    #[serde(default)]
    pub registration: RegistrationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebAuthnConfig {
    #[serde(default = "default_rp_id")]
    pub rp_id: String,
    #[serde(default = "default_rp_name")]
    pub rp_name: String,
    #[serde(default = "default_rp_origin")]
    pub rp_origin: String,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}

impl Default for WebAuthnConfig {
    fn default() -> Self {
        Self {
            rp_id: default_rp_id(),
            rp_name: default_rp_name(),
            rp_origin: default_rp_origin(),
            allowed_origins: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JwtConfig {
    #[serde(default = "default_jwt_issuer")]
    pub issuer: String,
    #[serde(default = "default_jwt_secret")]
    pub secret: String,
    #[serde(default = "default_jwt_expiration_hours")]
    pub expiration_hours: u64,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            issuer: default_jwt_issuer(),
            secret: default_jwt_secret(),
            expiration_hours: default_jwt_expiration_hours(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RegistrationConfig {
    #[serde(default)]
    pub allow_open_registration: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpstreamConfig {
    pub name: String,
    pub provider: String,
    pub base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default, skip_serializing)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TlsForwardConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_tlsforward_addresses")]
    pub tlsforward_addresses: Vec<String>,
    #[serde(default = "default_tlsforward_max_connections")]
    pub max_connections: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_key_path: Option<String>,
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval: u64,
}

impl Default for TlsForwardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            tlsforward_addresses: default_tlsforward_addresses(),
            max_connections: default_tlsforward_max_connections(),
            secret_key_path: None,
            heartbeat_interval: default_heartbeat_interval(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LetsEncryptConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default)]
    pub staging: bool,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default = "default_auto_renew_days")]
    pub auto_renew_days: u32,
}

impl Default for LetsEncryptConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            email: None,
            staging: false,
            domains: Vec::new(),
            auto_renew_days: default_auto_renew_days(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalInferenceConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_concurrent_inferences")]
    pub max_concurrent_inferences: usize,
    #[serde(default = "default_temperature")]
    pub default_temperature: f32,
    #[serde(default = "default_max_tokens")]
    pub default_max_tokens: u32,
}

impl Default for LocalInferenceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent_inferences: default_max_concurrent_inferences(),
            default_temperature: default_temperature(),
            default_max_tokens: default_max_tokens(),
        }
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    31145
}

fn default_true() -> bool {
    true
}

fn default_rp_id() -> String {
    "localhost".to_string()
}

fn default_rp_name() -> String {
    "Gate Self-Hosted".to_string()
}

fn default_rp_origin() -> String {
    format!("http://localhost:{}", default_port())
}

fn default_jwt_issuer() -> String {
    "gate-daemon".to_string()
}

fn default_jwt_secret() -> String {
    "change-me-in-production".to_string()
}

fn default_jwt_expiration_hours() -> u64 {
    24
}

fn default_timeout() -> u64 {
    30
}

fn default_tlsforward_addresses() -> Vec<String> {
    vec![
        "3dbefb2e3d56c7e32586d9a82167a8a5151f3e0f4b40b7c3d145b9060dde2f14@213.239.212.173:31145"
            .to_string(),
    ]
}

fn default_tlsforward_max_connections() -> usize {
    1000
}

fn default_heartbeat_interval() -> u64 {
    30
}

fn default_auto_renew_days() -> u32 {
    30
}

fn default_max_concurrent_inferences() -> usize {
    4
}

fn default_temperature() -> f32 {
    0.7
}

fn default_max_tokens() -> u32 {
    2048
}

#[function_component(ConfigEditor)]
pub fn config_editor() -> Html {
    let config_service = use_memo((), |_| ConfigApiService::new());
    let config = use_state(GateConfig::default);
    let is_loading = use_state(|| false);
    let is_saving = use_state(|| false);
    let error_message = use_state(|| None::<String>);
    let success_message = use_state(|| None::<String>);

    {
        let config_service = config_service.clone();
        let config = config.clone();
        let is_loading = is_loading.clone();
        let error_message = error_message.clone();

        use_effect_with((), move |_| {
            is_loading.set(true);
            wasm_bindgen_futures::spawn_local(async move {
                match config_service.get_config().await {
                    Ok(config_json) => match serde_json::from_value::<GateConfig>(config_json) {
                        Ok(loaded_config) => config.set(loaded_config),
                        Err(e) => error_message.set(Some(format!("Failed to parse config: {e}"))),
                    },
                    Err(e) => error_message.set(Some(format!("Failed to load config: {e}"))),
                }
                is_loading.set(false);
            });
        });
    }

    let on_save = {
        let config_service = config_service.clone();
        let config = config.clone();
        let is_saving = is_saving.clone();
        let error_message = error_message.clone();
        let success_message = success_message.clone();

        Callback::from(move |_| {
            is_saving.set(true);
            error_message.set(None);

            let config_json = match serde_json::to_value(&*config) {
                Ok(json) => json,
                Err(e) => {
                    error_message.set(Some(format!("Failed to serialize config: {e}")));
                    is_saving.set(false);
                    return;
                }
            };

            let config_service = config_service.clone();
            let is_saving = is_saving.clone();
            let error_message = error_message.clone();
            let success_message = success_message.clone();

            wasm_bindgen_futures::spawn_local(async move {
                match config_service.update_config(config_json).await {
                    Ok(_) => {
                        success_message.set(Some("Configuration saved successfully!".to_string()));
                        let success_message = success_message.clone();
                        Timeout::new(3000, move || {
                            success_message.set(None);
                        })
                        .forget();
                    }
                    Err(e) => error_message.set(Some(format!("Failed to save config: {e}"))),
                }
                is_saving.set(false);
            });
        })
    };

    let on_server_change = {
        let config = config.clone();
        Callback::from(move |new_server| {
            let mut new_config = (*config).clone();
            new_config.server = new_server;
            config.set(new_config);
        })
    };

    let on_auth_change = {
        let config = config.clone();
        Callback::from(move |new_auth| {
            let mut new_config = (*config).clone();
            new_config.auth = new_auth;
            config.set(new_config);
        })
    };

    let on_upstreams_change = {
        let config = config.clone();
        Callback::from(move |new_upstreams| {
            let mut new_config = (*config).clone();
            new_config.upstreams = new_upstreams;
            config.set(new_config);
        })
    };

    let on_tlsforward_change = {
        let config = config.clone();
        Callback::from(move |new_tlsforward| {
            let mut new_config = (*config).clone();
            new_config.tlsforward = new_tlsforward;
            config.set(new_config);
        })
    };

    let on_letsencrypt_change = {
        let config = config.clone();
        Callback::from(move |new_letsencrypt| {
            let mut new_config = (*config).clone();
            new_config.letsencrypt = new_letsencrypt;
            config.set(new_config);
        })
    };

    let on_inference_change = {
        let config = config.clone();
        Callback::from(move |new_inference| {
            let mut new_config = (*config).clone();
            new_config.local_inference = new_inference;
            config.set(new_config);
        })
    };

    html! {
        <div class="p-6 max-w-6xl mx-auto">
            <div class="bg-white dark:bg-gray-800 rounded-lg shadow-lg">
                <div class="border-b border-gray-200 dark:border-gray-700 px-6 py-4">
                    <h2 class="text-xl font-semibold text-gray-800 dark:text-gray-200">
                        {"Configuration Editor"}
                    </h2>
                    <p class="text-sm text-gray-600 dark:text-gray-400 mt-1">
                        {"Manage Gate configuration settings"}
                    </p>
                </div>

                <div class="p-6">
                    if *is_loading {
                        <div class="flex justify-center items-center h-64">
                            <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500"></div>
                        </div>
                    } else {
                        <>
                            if let Some(error) = (*error_message).as_ref() {
                                <div class="mb-4 p-4 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-md">
                                    <p class="text-red-700 dark:text-red-300">{error}</p>
                                </div>
                            }

                            if let Some(success) = (*success_message).as_ref() {
                                <div class="mb-4 p-4 bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800 rounded-md">
                                    <p class="text-green-700 dark:text-green-300">{success}</p>
                                </div>
                            }

                            <div class="space-y-4">
                                <ServerConfigSection
                                    config={config.server.clone()}
                                    on_change={on_server_change}
                                />
                                <AuthConfigSection
                                    config={config.auth.clone()}
                                    on_change={on_auth_change}
                                />
                                <UpstreamsConfigSection
                                    upstreams={config.upstreams.clone()}
                                    on_change={on_upstreams_change}
                                />
                                <TlsForwardConfigSection
                                    config={config.tlsforward.clone()}
                                    on_change={on_tlsforward_change}
                                />
                                <LetsEncryptConfigSection
                                    config={config.letsencrypt.clone()}
                                    on_change={on_letsencrypt_change}
                                />
                                <InferenceConfigSection
                                    config={config.local_inference.clone()}
                                    on_change={on_inference_change}
                                />
                            </div>

                            <div class="mt-6 flex justify-end">
                                <button
                                    class="px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                                    onclick={on_save}
                                    disabled={*is_saving}
                                >
                                    if *is_saving {
                                        {"Saving..."}
                                    } else {
                                        {"Save Configuration"}
                                    }
                                </button>
                            </div>
                        </>
                    }
                </div>
            </div>
        </div>
    }
}
