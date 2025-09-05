use super::provider_registry::ProviderMetadata;
use yew::prelude::*;

#[derive(Properties)]
pub struct ProviderCardProps {
    pub provider: &'static ProviderMetadata,
    pub is_configured: bool,
    pub on_click: Callback<&'static str>,
}

impl PartialEq for ProviderCardProps {
    fn eq(&self, other: &Self) -> bool {
        self.provider.id == other.provider.id && self.is_configured == other.is_configured
    }
}

#[function_component(ProviderCard)]
pub fn provider_card(props: &ProviderCardProps) -> Html {
    let onclick = {
        let provider_id = props.provider.id;
        let on_click = props.on_click.clone();
        Callback::from(move |_| {
            on_click.emit(provider_id);
        })
    };

    html! {
        <button
            class={classes!(
                "relative", "group", "p-4", "rounded-lg", "border-2",
                "transition-all", "duration-200", "flex", "flex-col",
                "items-center", "justify-center", "gap-2", "cursor-pointer",
                "hover:scale-105", "hover:shadow-lg",
                if props.is_configured {
                    "border-blue-500 dark:border-blue-400 bg-blue-50 dark:bg-blue-900/20"
                } else {
                    "border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 hover:border-gray-300 dark:hover:border-gray-600"
                }
            )}
            onclick={onclick}
            type="button"
        >
            // Configured indicator
            if props.is_configured {
                <div class="absolute top-2 right-2">
                    <svg class="w-5 h-5 text-blue-500 dark:text-blue-400" fill="currentColor" viewBox="0 0 20 20">
                        <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd"/>
                    </svg>
                </div>
            }

            // Provider icon
            <div class="w-16 h-16 flex items-center justify-center">
                <img
                    src={props.provider.icon_path}
                    alt={props.provider.display_name}
                    class="w-full h-full object-contain rounded-lg"
                />
            </div>

            // Provider name
            <span class={classes!(
                "text-sm", "font-medium", "text-center",
                if props.is_configured {
                    "text-blue-700 dark:text-blue-300"
                } else {
                    "text-gray-700 dark:text-gray-300"
                }
            )}>
                {props.provider.display_name}
            </span>

            // Hover overlay
            <div class="absolute inset-0 rounded-lg bg-black opacity-0 group-hover:opacity-5 transition-opacity pointer-events-none"></div>
        </button>
    }
}
