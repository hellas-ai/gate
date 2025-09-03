use super::super::{inference::InferenceConfigSection, types::LocalInferenceConfig};
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct InferenceConfigPageProps {
    pub config: Option<LocalInferenceConfig>,
    pub on_change: Callback<Option<LocalInferenceConfig>>,
}

#[function_component(InferenceConfigPage)]
pub fn inference_config_page(props: &InferenceConfigPageProps) -> Html {
    html! {
        <div class="p-6">
            <div class="mb-6">
                <h2 class="text-lg font-semibold text-gray-900 dark:text-gray-100">
                    {"Local Inference Settings"}
                </h2>
                <p class="mt-1 text-sm text-gray-600 dark:text-gray-400">
                    {"Configure local AI model inference parameters and resource limits"}
                </p>
            </div>

            <InferenceConfigSection
                config={props.config.clone()}
                on_change={props.on_change.clone()}
            />
        </div>
    }
}
