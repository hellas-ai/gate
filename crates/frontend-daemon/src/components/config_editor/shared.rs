use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct ConfigSectionProps {
    pub title: String,
    pub children: Html,
    #[prop_or_default]
    pub enabled: Option<bool>,
    #[prop_or_default]
    pub on_toggle: Option<Callback<bool>>,
}

#[function_component(ConfigSection)]
pub fn config_section(props: &ConfigSectionProps) -> Html {
    let is_enabled = props.enabled.unwrap_or(true);
    let expanded = use_state(|| is_enabled);

    // Auto-collapse when disabled, auto-expand when enabled
    {
        let expanded = expanded.clone();
        use_effect_with(is_enabled, move |enabled| {
            expanded.set(*enabled);
            || ()
        });
    }

    let toggle_expanded = {
        let expanded = expanded.clone();
        Callback::from(move |e: MouseEvent| {
            e.prevent_default();
            expanded.set(!*expanded);
        })
    };

    let on_toggle_enabled = {
        let on_toggle = props.on_toggle.clone();
        Callback::from(move |e: MouseEvent| {
            e.stop_propagation();
            if let Some(callback) = &on_toggle {
                callback.emit(!is_enabled);
            }
        })
    };

    html! {
        <div class={classes!(
            "border", "rounded-lg", "mb-3", "transition-all", "duration-300",
            if is_enabled {
                "border-gray-200 dark:border-gray-700"
            } else {
                "border-gray-200/50 dark:border-gray-700/50 opacity-60"
            }
        )}>
            <div
                class={classes!(
                    "px-4", "py-2.5", "flex", "items-center", "justify-between", "transition-colors", "cursor-pointer",
                    if is_enabled {
                        "hover:bg-gray-50 dark:hover:bg-gray-800"
                    } else {
                        "bg-gray-50/50 dark:bg-gray-800/50"
                    }
                )}
                onclick={toggle_expanded.clone()}
            >
                <h3 class={classes!(
                    "text-base", "font-medium",
                    if is_enabled {
                        "text-gray-900 dark:text-gray-100"
                    } else {
                        "text-gray-500 dark:text-gray-400"
                    }
                )}>
                    {&props.title}
                </h3>
                <div class="flex items-center gap-3">
                    {if props.on_toggle.is_some() {
                        html! {
                            <div class="flex items-center gap-2">
                                <span class="text-sm text-gray-600 dark:text-gray-400">{"Enable"}</span>
                                <button
                                    type="button"
                                    role="switch"
                                    aria-checked={is_enabled.to_string()}
                                    onclick={on_toggle_enabled}
                                    class={classes!(
                                        "relative", "inline-flex", "h-5", "w-9", "items-center", "rounded-full",
                                        "transition-colors", "focus:outline-none", "focus:ring-2",
                                        "focus:ring-blue-500", "focus:ring-offset-1",
                                        if is_enabled {
                                            "bg-blue-600"
                                        } else {
                                            "bg-gray-300 dark:bg-gray-600"
                                        }
                                    )}
                                >
                                    <span
                                        class={classes!(
                                            "inline-block", "h-3.5", "w-3.5", "rounded-full", "bg-white",
                                            "transition-transform", "duration-200",
                                            if is_enabled {
                                                "translate-x-5"
                                            } else {
                                                "translate-x-1"
                                            }
                                        )}
                                    />
                                </button>
                            </div>
                        }
                    } else {
                        html! {
                            <svg
                                class={classes!(
                                    "w-4", "h-4", "text-gray-500", "transition-transform", "duration-200",
                                    if *expanded { "rotate-180" } else { "" }
                                )}
                                fill="none"
                                stroke="currentColor"
                                viewBox="0 0 24 24"
                            >
                                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
                            </svg>
                        }
                    }}
                </div>
            </div>
            <div
                class={classes!(
                    "overflow-hidden", "transition-all", "duration-300",
                    if *expanded {
                        "max-h-[2000px] opacity-100"
                    } else {
                        "max-h-0 opacity-0"
                    }
                )}
            >
                <div class={classes!(
                    "px-4", "py-3", "border-t",
                    if is_enabled {
                        "border-gray-200 dark:border-gray-700"
                    } else {
                        "border-gray-200/50 dark:border-gray-700/50 pointer-events-none"
                    }
                )}>
                    {props.children.clone()}
                </div>
            </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct ConfigFieldProps {
    pub label: String,
    pub children: Html,
    #[prop_or_default]
    pub help_text: Option<String>,
    #[prop_or(false)]
    pub full_width: bool,
}

#[function_component(ConfigField)]
pub fn config_field(props: &ConfigFieldProps) -> Html {
    if props.full_width {
        html! {
            <div class="mb-3">
                <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                    {&props.label}
                </label>
                {props.children.clone()}
                if let Some(help) = &props.help_text {
                    <p class="mt-1 text-xs text-gray-500 dark:text-gray-400">{help}</p>
                }
            </div>
        }
    } else {
        html! {
            <div class="mb-3 sm:flex sm:items-start sm:gap-4">
                <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1 sm:mb-0 sm:w-1/3 sm:pt-2">
                    {&props.label}
                    if let Some(help) = &props.help_text {
                        <p class="text-xs text-gray-500 dark:text-gray-400 font-normal mt-0.5">{help}</p>
                    }
                </label>
                <div class="sm:flex-1">
                    {props.children.clone()}
                </div>
            </div>
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct ConfigToggleProps {
    pub label: String,
    pub checked: bool,
    pub on_change: Callback<bool>,
    #[prop_or_default]
    pub help_text: Option<String>,
}

#[function_component(ConfigToggle)]
pub fn config_toggle(props: &ConfigToggleProps) -> Html {
    let onclick = {
        let on_change = props.on_change.clone();
        let checked = props.checked;
        Callback::from(move |_| on_change.emit(!checked))
    };

    html! {
        <div class="mb-3 sm:flex sm:items-start sm:gap-4">
            <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1 sm:mb-0 sm:w-1/3 sm:pt-2">
                {&props.label}
                if let Some(help) = &props.help_text {
                    <p class="text-xs text-gray-500 dark:text-gray-400 font-normal mt-0.5">{help}</p>
                }
            </label>
            <div class="sm:flex-1">
                <button
                    type="button"
                    role="switch"
                    aria-checked={props.checked.to_string()}
                    onclick={onclick}
                    class={classes!(
                        "relative", "inline-flex", "h-6", "w-11", "items-center", "rounded-full",
                        "transition-colors", "focus:outline-none", "focus:ring-2",
                        "focus:ring-blue-500", "focus:ring-offset-2",
                        if props.checked {
                            "bg-blue-600"
                        } else {
                            "bg-gray-200 dark:bg-gray-700"
                        }
                    )}
                >
                    <span
                        class={classes!(
                            "inline-block", "h-4", "w-4", "rounded-full", "bg-white",
                            "transition-transform",
                            if props.checked {
                                "translate-x-6"
                            } else {
                                "translate-x-1"
                            }
                        )}
                    />
                </button>
            </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct ConfigInputProps {
    pub value: String,
    pub on_change: Callback<String>,
    #[prop_or("text".to_string())]
    pub input_type: String,
    #[prop_or_default]
    pub placeholder: Option<String>,
    #[prop_or(false)]
    pub disabled: bool,
}

#[function_component(ConfigInput)]
pub fn config_input(props: &ConfigInputProps) -> Html {
    let oninput = {
        let on_change = props.on_change.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            on_change.emit(input.value());
        })
    };

    html! {
        <input
            type={props.input_type.clone()}
            value={props.value.clone()}
            oninput={oninput}
            placeholder={props.placeholder.clone().unwrap_or_default()}
            disabled={props.disabled}
            class="w-full px-2.5 py-1.5 text-sm border border-gray-300 dark:border-gray-600 rounded-md shadow-sm
                   focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500
                   bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100
                   disabled:bg-gray-100 dark:disabled:bg-gray-900 disabled:cursor-not-allowed"
        />
    }
}

#[derive(Properties, PartialEq)]
pub struct ConfigSelectProps {
    pub value: String,
    pub options: Vec<(String, String)>,
    pub on_change: Callback<String>,
    #[prop_or(false)]
    pub disabled: bool,
}

#[function_component(ConfigSelect)]
pub fn config_select(props: &ConfigSelectProps) -> Html {
    let onchange = {
        let on_change = props.on_change.clone();
        Callback::from(move |e: Event| {
            let select: web_sys::HtmlSelectElement = e.target_unchecked_into();
            on_change.emit(select.value());
        })
    };

    html! {
        <select
            value={props.value.clone()}
            onchange={onchange}
            disabled={props.disabled}
            class="w-full px-2.5 py-1.5 text-sm border border-gray-300 dark:border-gray-600 rounded-md shadow-sm
                   focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500
                   bg-white dark:bg-gray-800 text-gray-900 dark:text-gray-100
                   disabled:bg-gray-100 dark:disabled:bg-gray-900 disabled:cursor-not-allowed"
        >
            {props.options.iter().map(|(value, label)| {
                html! {
                    <option value={value.clone()} selected={value == &props.value}>
                        {label}
                    </option>
                }
            }).collect::<Html>()}
        </select>
    }
}
