use yew::prelude::*;

use super::container::LocalInferenceConfig;
use super::shared::{ConfigField, ConfigInput, ConfigSection};

#[derive(Properties, PartialEq)]
pub struct InferenceConfigSectionProps {
    pub config: Option<LocalInferenceConfig>,
    pub on_change: Callback<Option<LocalInferenceConfig>>,
}

#[function_component(InferenceConfigSection)]
pub fn inference_config_section(props: &InferenceConfigSectionProps) -> Html {
    let enabled = props.config.is_some();
    let config = props.config.clone().unwrap_or_default();

    let on_enabled_toggle = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: bool| {
            if value {
                on_change.emit(Some(config.clone()));
            } else {
                on_change.emit(None);
            }
        })
    };

    let on_max_concurrent_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            if let Ok(max_concurrent) = value.parse::<usize>() {
                let mut new_config = config.clone();
                new_config.max_concurrent_inferences = max_concurrent;
                on_change.emit(Some(new_config));
            }
        })
    };

    let on_temperature_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            if let Ok(temp) = value.parse::<f32>() {
                if (0.0..=2.0).contains(&temp) {
                    let mut new_config = config.clone();
                    new_config.default_temperature = temp;
                    on_change.emit(Some(new_config));
                }
            }
        })
    };

    let on_max_tokens_change = {
        let config = config.clone();
        let on_change = props.on_change.clone();
        Callback::from(move |value: String| {
            if let Ok(max_tokens) = value.parse::<u32>() {
                let mut new_config = config.clone();
                new_config.default_max_tokens = max_tokens;
                on_change.emit(Some(new_config));
            }
        })
    };

    html! {
        <ConfigSection
            title="Local Inference Configuration"
            enabled={enabled}
            on_toggle={on_enabled_toggle}
        >
            <ConfigField
                label="Maximum Concurrent Inferences"
                help_text="Number of inference requests that can be processed simultaneously"
            >
                <ConfigInput
                    value={config.max_concurrent_inferences.to_string()}
                    on_change={on_max_concurrent_change}
                    input_type="number"
                    placeholder="4"
                />
            </ConfigField>

            <ConfigField
                label="Default Temperature"
                help_text="Controls randomness in model output (0.0 - 2.0)"
            >
                <ConfigInput
                    value={config.default_temperature.to_string()}
                    on_change={on_temperature_change}
                    input_type="number"
                    placeholder="0.7"
                />
            </ConfigField>

            <ConfigField
                label="Default Maximum Tokens"
                help_text="Maximum number of tokens to generate in responses"
            >
                <ConfigInput
                    value={config.default_max_tokens.to_string()}
                    on_change={on_max_tokens_change}
                    input_type="number"
                    placeholder="2048"
                />
            </ConfigField>
        </ConfigSection>
    }
}
