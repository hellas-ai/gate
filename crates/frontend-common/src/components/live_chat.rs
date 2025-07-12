use crate::services::{ChatMessage, InferenceService, Model, Role};
use gate_chat_ui::{
    types::{ChatMessage as UIChatMessage, ChatResponse, Provider as UIProvider},
    ChatContainer,
};
use std::collections::HashMap;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlSelectElement;
use yew::prelude::*;

#[function_component(LiveChat)]
pub fn live_chat() -> Html {
    let messages = use_state(Vec::<UIChatMessage>::new);
    let loading = use_state(|| false);
    let error = use_state(|| None::<String>);
    let selected_model = use_state(|| None::<String>);
    let available_models = use_state(Vec::<Model>::new);
    let models_loading = use_state(|| true); // Start as true to indicate initial load
    let show_settings = use_state(|| true);
    let manual_model_input = use_state(String::new);
    let use_manual_model = use_state(|| false);

    // Get the current model name for display
    let current_model = if *use_manual_model {
        if manual_model_input.is_empty() {
            "No model entered".to_string()
        } else {
            (*manual_model_input).clone()
        }
    } else {
        selected_model
            .as_ref()
            .and_then(|id| available_models.iter().find(|m| m.id == *id))
            .map(|m| m.id.clone())
            .unwrap_or_else(|| "No model selected".to_string())
    };

    // Convert our messages to chat response format
    let chat_response = ChatResponse {
        id: "live-chat".to_string(),
        provider: UIProvider::OpenAI,
        model: current_model.clone(),
        messages: (*messages).clone(),
        usage: None,
        metadata: HashMap::new(),
    };

    // Fetch models on component mount
    {
        let available_models = available_models.clone();
        let models_loading = models_loading.clone();
        let error = error.clone();
        let selected_model = selected_model.clone();

        use_effect_with((), move |_| {
            spawn_local(async move {
                models_loading.set(true);
                match InferenceService::get_models().await {
                    Ok(models) => {
                        web_sys::console::log_1(&format!("Fetched {} models", models.len()).into());

                        // Set Qwen as default if available
                        if selected_model.is_none() {
                            if let Some(qwen_model) = models.iter().find(|m| m.id.contains("Qwen"))
                            {
                                selected_model.set(Some(qwen_model.id.clone()));
                                web_sys::console::log_1(
                                    &format!("Set default model to: {}", qwen_model.id).into(),
                                );
                            }
                        }

                        available_models.set(models);
                    }
                    Err(e) => {
                        web_sys::console::error_1(&format!("Failed to fetch models: {e}").into());
                        error.set(Some(format!("Failed to fetch models: {e}")));
                    }
                }
                models_loading.set(false);
            });
        });
    }

    let on_send_message = {
        let messages = messages.clone();
        let loading = loading.clone();
        let error = error.clone();
        let selected_model = selected_model.clone();
        let manual_model_input = manual_model_input.clone();
        let use_manual_model = use_manual_model.clone();

        Callback::from(move |user_message: String| {
            if user_message.trim().is_empty() || *loading {
                return;
            }

            let model = if *use_manual_model {
                let manual_model = (*manual_model_input).clone();
                if manual_model.trim().is_empty() {
                    error.set(Some("Please enter a model name".to_string()));
                    return;
                }
                manual_model
            } else {
                match &*selected_model {
                    Some(m) => m.clone(),
                    None => {
                        error.set(Some(
                            "Please select a model or enter one manually".to_string(),
                        ));
                        return;
                    }
                }
            };

            let messages = messages.clone();
            let loading = loading.clone();
            let error = error.clone();

            spawn_local(async move {
                loading.set(true);
                error.set(None);

                // Add user message to the chat
                let mut updated_messages = (*messages).clone();
                updated_messages.push(UIChatMessage::user(user_message.clone()));
                messages.set(updated_messages.clone());

                // Prepare messages for API call
                let api_messages: Vec<ChatMessage> = updated_messages
                    .iter()
                    .map(|msg| ChatMessage {
                        role: match msg.role.as_str() {
                            "system" => Role::System,
                            "user" => Role::User,
                            "assistant" => Role::Assistant,
                            _ => Role::User,
                        },
                        content: msg.get_text_content().unwrap_or_default(),
                    })
                    .collect();

                // Detect provider from model
                let provider = InferenceService::detect_provider(&model);

                // Make API call
                match InferenceService::chat_completion(
                    provider,
                    model.clone(),
                    api_messages,
                    Some(0.7),
                    Some(1000),
                )
                .await
                {
                    Ok(response) => {
                        web_sys::console::log_1(&format!("API Response: {response:?}").into());

                        // Parse response based on provider
                        if let Some(assistant_message) =
                            InferenceService::parse_response(provider, &response)
                        {
                            let mut final_messages = updated_messages.clone();
                            final_messages.push(UIChatMessage::assistant(assistant_message));
                            messages.set(final_messages);
                        } else {
                            error.set(Some("Failed to parse response".to_string()));
                            web_sys::console::error_1(
                                &format!("Failed to parse response: {response:?}").into(),
                            );
                        }
                    }
                    Err(e) => {
                        error.set(Some(format!("API Error: {e}")));
                        web_sys::console::error_1(&format!("API Error: {e}").into());
                        // Remove the user message if the API call failed
                        messages.set((*messages).clone());
                    }
                }

                loading.set(false);
            });
        })
    };

    let on_model_change = {
        let selected_model = selected_model.clone();
        Callback::from(move |e: Event| {
            if let Some(select) = e.target_dyn_into::<HtmlSelectElement>() {
                let value = select.value();
                if value.is_empty() {
                    selected_model.set(None);
                } else {
                    selected_model.set(Some(value));
                }
            }
        })
    };

    let toggle_settings = {
        let show_settings = show_settings.clone();
        Callback::from(move |_| {
            show_settings.set(!*show_settings);
        })
    };

    let clear_chat = {
        let messages = messages.clone();
        let error = error.clone();
        Callback::from(move |_| {
            messages.set(Vec::new());
            error.set(None);
        })
    };

    let on_manual_model_change = {
        let manual_model_input = manual_model_input.clone();
        Callback::from(move |e: InputEvent| {
            if let Some(input) = e.target_dyn_into::<web_sys::HtmlInputElement>() {
                manual_model_input.set(input.value());
            }
        })
    };

    let on_toggle_manual_model = {
        let use_manual_model = use_manual_model.clone();
        Callback::from(move |_| {
            use_manual_model.set(!*use_manual_model);
        })
    };

    html! {
        <div class="flex h-[calc(100vh-100px)] gap-4 p-4 bg-gray-100 dark:bg-gray-900">
            if *show_settings {
                <div class="w-[300px] bg-white dark:bg-gray-800 rounded-lg p-5 shadow-md overflow-y-auto">
                    <div class="flex justify-between items-center mb-4">
                        <h2 class="text-xl font-bold text-gray-800 dark:text-gray-200">{"Live Chat Settings"}</h2>
                        <button
                            onclick={toggle_settings.clone()}
                            class="text-gray-600 dark:text-gray-400 hover:text-gray-800 dark:hover:text-gray-200 transition-colors"
                            title="Hide sidebar"
                        >
                            {"× Sidebar"}
                        </button>
                    </div>

                    <div class="mb-4">
                        <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
                            {"Model"}
                        </label>

                        // Toggle button for manual model input
                        <div class="mb-2">
                            <button
                                onclick={on_toggle_manual_model}
                                class="text-sm text-blue-600 dark:text-blue-400 hover:underline"
                            >
                                {if *use_manual_model { "← Use model list" } else { "Enter model manually →" }}
                            </button>
                        </div>

                        if *use_manual_model {
                            // Manual model input
                            <input
                                type="text"
                                placeholder="e.g., gpt-4, claude-3-opus-20240229"
                                value={(*manual_model_input).clone()}
                                oninput={on_manual_model_change}
                                class="w-full p-2 border border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-200 rounded text-sm"
                            />
                        } else {
                            // Model selector
                            if *models_loading {
                                <div class="w-full p-2 border border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-400 rounded text-sm">
                                    {"Loading models..."}
                                </div>
                            } else if available_models.is_empty() {
                                <div class="w-full p-2 border border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-400 rounded text-sm">
                                    {"No models available"}
                                </div>
                            } else {
                                <select
                                    onchange={on_model_change}
                                    class="w-full p-2 border border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-200 rounded text-sm"
                                    value={selected_model.as_ref().cloned().unwrap_or_default()}
                                >
                                    <option value="">{"Select a model"}</option>
                                    {available_models.iter().map(|model| {
                                        html! {
                                            <option value={model.id.clone()} selected={Some(&model.id) == selected_model.as_ref()}>
                                                {format!("{} ({})", model.id, model.owned_by)}
                                            </option>
                                        }
                                    }).collect::<Html>()}
                                </select>
                            }
                            if !available_models.is_empty() {
                                <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                                    {format!("{} models available", available_models.len())}
                                </p>
                            }
                        }
                    </div>

                    <button
                        onclick={clear_chat}
                        class="w-full bg-gray-200 hover:bg-gray-300 dark:bg-gray-700 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 px-4 py-2 rounded text-sm transition-colors mb-4"
                    >
                        {"Clear Chat"}
                    </button>

                    if let Some(err) = &*error {
                        <div class="bg-red-50 dark:bg-red-900 text-red-700 dark:text-red-300 p-3 rounded text-sm mb-4">
                            {err}
                        </div>
                    }
                </div>
            }

            <div class="flex-1 bg-white dark:bg-gray-800 rounded-lg shadow-md overflow-hidden flex flex-col">
                <div class="p-3 bg-gray-50 dark:bg-gray-900 border-b border-gray-200 dark:border-gray-700 flex justify-between items-center">
                    <div class="flex items-center gap-2">
                        if !*show_settings {
                            <button
                                onclick={toggle_settings}
                                class="text-gray-600 dark:text-gray-400 hover:text-gray-800 dark:hover:text-gray-200 transition-colors"
                            >
                                {"☰ Sidebar"}
                            </button>
                        }
                    </div>
                    if *loading {
                        <span class="text-sm text-gray-500 dark:text-gray-400">{"Loading..."}</span>
                    }
                </div>

                <ChatContainer
                    chat_response={chat_response}
                    show_metadata={false}
                    show_input={true}
                    on_send_message={on_send_message}
                    input_disabled={*loading}
                />
            </div>
        </div>
    }
}
