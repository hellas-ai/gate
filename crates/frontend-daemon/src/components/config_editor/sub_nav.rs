use yew::prelude::*;

#[derive(Clone, Copy, PartialEq)]
pub enum ConfigPage {
    Server,
    Authentication,
    Providers,
    Network,
    Inference,
}

impl ConfigPage {
    pub fn label(&self) -> &'static str {
        match self {
            ConfigPage::Server => "Server",
            ConfigPage::Authentication => "Authentication",
            ConfigPage::Providers => "Providers",
            ConfigPage::Network => "Network",
            ConfigPage::Inference => "Inference",
        }
    }

    pub fn icon(&self) -> Html {
        match self {
            ConfigPage::Server => html! {
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2m-2-4h.01M17 16h.01"></path>
                </svg>
            },
            ConfigPage::Authentication => html! {
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z"></path>
                </svg>
            },
            ConfigPage::Providers => html! {
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z"></path>
                </svg>
            },
            ConfigPage::Network => html! {
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 12a9 9 0 01-9 9m9-9a9 9 0 00-9-9m9 9H3m9 9a9 9 0 01-9-9m9 9c1.657 0 3-4.03 3-9s-1.343-9-3-9m0 18c-1.657 0-3-4.03-3-9s1.343-9 3-9m-9 9a9 9 0 019-9"></path>
                </svg>
            },
            ConfigPage::Inference => html! {
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z"></path>
                </svg>
            },
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            ConfigPage::Server => "Configure server host, port, and metrics settings",
            ConfigPage::Authentication => "Manage authentication, WebAuthn, and JWT settings",
            ConfigPage::Providers => "Configure AI provider connections and API keys",
            ConfigPage::Network => "Setup TLS forwarding and Let's Encrypt certificates",
            ConfigPage::Inference => "Configure local inference settings and parameters",
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct SubNavProps {
    pub active_page: ConfigPage,
    pub on_change: Callback<ConfigPage>,
}

#[function_component(SubNav)]
pub fn sub_nav(props: &SubNavProps) -> Html {
    let pages = [
        ConfigPage::Server,
        ConfigPage::Authentication,
        ConfigPage::Providers,
        ConfigPage::Network,
        ConfigPage::Inference,
    ];

    html! {
        <div class="border-b border-gray-200 dark:border-gray-700">
            // Tab-style navigation
            <div class="flex gap-1 px-4">
                {pages.iter().map(|page| {
                    let is_active = *page == props.active_page;
                    let onclick = {
                        let page = *page;
                        let on_change = props.on_change.clone();
                        Callback::from(move |_| on_change.emit(page))
                    };

                    html! {
                        <button
                            class={classes!(
                                "px-4", "py-3", "flex", "items-center", "gap-2",
                                "text-sm", "font-medium", "border-b-2", "transition-colors",
                                "-mb-px", // Overlap the container border
                                if is_active {
                                    "text-blue-600 dark:text-blue-400 border-blue-600 dark:border-blue-400"
                                } else {
                                    "text-gray-600 dark:text-gray-400 border-transparent hover:text-gray-900 dark:hover:text-gray-100"
                                }
                            )}
                            onclick={onclick}
                            title={page.description()}
                        >
                            {page.icon()}
                            <span>{page.label()}</span>
                        </button>
                    }
                }).collect::<Html>()}
            </div>

            // Alternative: Pill-style navigation (commented out)
            // <div class="flex gap-2 px-4 py-3">
            //     {pages.iter().map(|page| {
            //         let is_active = *page == props.active_page;
            //         let onclick = {
            //             let page = *page;
            //             let on_change = props.on_change.clone();
            //             Callback::from(move |_| on_change.emit(page))
            //         };
            //
            //         html! {
            //             <button
            //                 class={classes!(
            //                     "px-4", "py-2", "flex", "items-center", "gap-2",
            //                     "text-sm", "font-medium", "rounded-lg", "transition-colors",
            //                     if is_active {
            //                         "bg-blue-100 dark:bg-blue-900 text-blue-700 dark:text-blue-300"
            //                     } else {
            //                         "text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800"
            //                     }
            //                 )}
            //                 onclick={onclick}
            //             >
            //                 {page.icon()}
            //                 <span>{page.label()}</span>
            //             </button>
            //         }
            //     }).collect::<Html>()}
            // </div>
        </div>
    }
}
