use gate_chat_ui::utils::simple_cassette_parser::parse_cassette;
use gate_chat_ui::{ChatContainer, ChatMessage, ChatResponse, MultimodalMessage, Provider};
use gloo_net::http::Request;
use serde_json::json;
use wasm_bindgen::prelude::*;
use web_sys::window;
use yew::prelude::*;

#[derive(Clone, PartialEq)]
struct CassetteInfo {
    provider: String,
    name: String,
    display_name: String,
    path: String,
}

#[function_component(App)]
fn app() -> Html {
    let dark_mode = use_state(|| {
        // Check if dark mode is already set
        window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
            .map(|e| e.class_list().contains("dark"))
            .unwrap_or(false)
    });

    let toggle_dark_mode = {
        let dark_mode = dark_mode.clone();
        Callback::from(move |_| {
            let new_state = !*dark_mode;
            dark_mode.set(new_state);

            // Update the document element class
            #[allow(clippy::collapsible_if)]
            if let Some(window) = window() {
                if let Some(document) = window.document() {
                    if let Some(element) = document.document_element() {
                        let class_list = element.class_list();
                        if new_state {
                            let _ = class_list.add_1("dark");
                        } else {
                            let _ = class_list.remove_1("dark");
                        }
                    }
                }
            }
        })
    };

    let chat_response = use_state(|| ChatResponse {
        id: "demo-1".to_string(),
        model: "demo-model".to_string(),
        messages: vec![
            ChatMessage::system("You are a helpful assistant."),
            ChatMessage::user("Hello! Can you help me understand how this chat UI works?"),
            ChatMessage::assistant(
                "Of course! This is a demo of the Gate Chat UI component. It provides a clean interface for displaying chat messages between users and AI assistants. The UI supports various message types including system messages, user messages, and assistant responses.",
            ),
        ],
        provider: Provider::Unknown("demo".to_string()),
        usage: None,
        metadata: Default::default(),
    });

    let loading_cassette = use_state(|| false);
    let cassette_error = use_state(|| None::<String>);

    // Hardcoded list of available cassettes
    // In production, this could be fetched from an API endpoint
    let available_cassettes = vec![
        // OpenAI Chat Completions
        CassetteInfo {
            provider: "openai".to_string(),
            name: "basic_chat_completion".to_string(),
            display_name: "Basic Chat Completion".to_string(),
            path: "/cassettes/openai/chat_completions/basic_chat_completion.json".to_string(),
        },
        CassetteInfo {
            provider: "openai".to_string(),
            name: "streaming_chat_completion".to_string(),
            display_name: "Streaming Chat Completion".to_string(),
            path: "/cassettes/openai/chat_completions/streaming_chat_completion.json".to_string(),
        },
        CassetteInfo {
            provider: "openai".to_string(),
            name: "multi_turn_conversation".to_string(),
            display_name: "Multi-turn Conversation".to_string(),
            path: "/cassettes/openai/chat_completions/multi_turn_conversation.json".to_string(),
        },
        CassetteInfo {
            provider: "openai".to_string(),
            name: "function_calling".to_string(),
            display_name: "Function Calling".to_string(),
            path: "/cassettes/openai/chat_completions/function_calling.json".to_string(),
        },
        CassetteInfo {
            provider: "openai".to_string(),
            name: "json_mode".to_string(),
            display_name: "JSON Mode".to_string(),
            path: "/cassettes/openai/chat_completions/json_mode.json".to_string(),
        },
        CassetteInfo {
            provider: "openai".to_string(),
            name: "system_message_instructions".to_string(),
            display_name: "System Message Instructions".to_string(),
            path: "/cassettes/openai/chat_completions/system_message_instructions.json".to_string(),
        },
        // Anthropic Messages
        CassetteInfo {
            provider: "anthropic".to_string(),
            name: "basic_message".to_string(),
            display_name: "Basic Message".to_string(),
            path: "/cassettes/anthropic/messages/basic_message.json".to_string(),
        },
        CassetteInfo {
            provider: "anthropic".to_string(),
            name: "streaming_basic".to_string(),
            display_name: "Streaming Basic".to_string(),
            path: "/cassettes/anthropic/messages/streaming_basic.json".to_string(),
        },
        CassetteInfo {
            provider: "anthropic".to_string(),
            name: "vision_image_analysis".to_string(),
            display_name: "Vision Image Analysis".to_string(),
            path: "/cassettes/anthropic/messages/vision_image_analysis.json".to_string(),
        },
        CassetteInfo {
            provider: "anthropic".to_string(),
            name: "multi_turn_conversation".to_string(),
            display_name: "Multi-turn Conversation".to_string(),
            path: "/cassettes/anthropic/messages/multi_turn_conversation.json".to_string(),
        },
        CassetteInfo {
            provider: "anthropic".to_string(),
            name: "tool_use_multi_turn".to_string(),
            display_name: "Tool Use Multi-turn".to_string(),
            path: "/cassettes/anthropic/messages/tool_use_multi_turn.json".to_string(),
        },
        CassetteInfo {
            provider: "anthropic".to_string(),
            name: "system_prompt".to_string(),
            display_name: "System Prompt".to_string(),
            path: "/cassettes/anthropic/messages/system_prompt.json".to_string(),
        },
        // OpenAI Responses
        CassetteInfo {
            provider: "openai".to_string(),
            name: "basic_response".to_string(),
            display_name: "Basic Response".to_string(),
            path: "/cassettes/openai/responses/basic_response.json".to_string(),
        },
        CassetteInfo {
            provider: "openai".to_string(),
            name: "basic_streaming".to_string(),
            display_name: "Basic Streaming Response".to_string(),
            path: "/cassettes/openai/responses/basic_streaming.json".to_string(),
        },
        CassetteInfo {
            provider: "openai".to_string(),
            name: "conversation_with_context".to_string(),
            display_name: "Conversation with Context".to_string(),
            path: "/cassettes/openai/responses/conversation_with_context.json".to_string(),
        },
    ];

    let on_cassette_select = {
        let chat_response = chat_response.clone();
        let loading_cassette = loading_cassette.clone();
        let cassette_error = cassette_error.clone();
        let available_cassettes = available_cassettes.clone();

        Callback::from(move |e: Event| {
            let target = e.target().unwrap();
            let select = target.dyn_into::<web_sys::HtmlSelectElement>().unwrap();
            let value = select.value();

            if value.is_empty() {
                return;
            }

            // Find the selected cassette
            let cassette = available_cassettes
                .iter()
                .find(|c| format!("{}:{}", c.provider, c.name) == value);

            if let Some(cassette_info) = cassette {
                loading_cassette.set(true);
                cassette_error.set(None);

                let chat_response = chat_response.clone();
                let loading_cassette = loading_cassette.clone();
                let cassette_error = cassette_error.clone();
                let path = cassette_info.path.clone();

                wasm_bindgen_futures::spawn_local(async move {
                    match Request::get(&path).send().await {
                        Ok(resp) => match resp.text().await {
                            Ok(text) => match parse_cassette(&text) {
                                Ok(parsed) => {
                                    chat_response.set(parsed);
                                }
                                Err(e) => {
                                    cassette_error
                                        .set(Some(format!("Failed to parse cassette: {e}")));
                                }
                            },
                            Err(e) => {
                                cassette_error.set(Some(format!("Failed to read response: {e}")));
                            }
                        },
                        Err(e) => {
                            cassette_error.set(Some(format!("Failed to load cassette: {e}")));
                        }
                    }
                    loading_cassette.set(false);
                });
            }

            // Reset select to placeholder
            select.set_value("");
        })
    };

    let on_send_multimodal = {
        let chat_response = chat_response.clone();
        Callback::from(move |message: MultimodalMessage| {
            let mut updated = (*chat_response).clone();

            // Convert multimodal message to chat message with proper content blocks
            let content_blocks = message.to_content_blocks();

            if !content_blocks.is_empty() {
                let user_message = ChatMessage {
                    role: "user".to_string(),
                    content: Some(json!(content_blocks)),
                    tool_calls: None,
                    name: None,
                    metadata: Default::default(),
                };
                updated.messages.push(user_message);

                // Simulate assistant response
                let response_text = if message.attachments.iter().any(|a| a.is_image()) {
                    "I can see the image(s) you've uploaded. This is a simulated response - in a real implementation, I would analyze the image content.".to_string()
                } else if !message.attachments.is_empty() {
                    format!(
                        "I received {} file(s). This is a simulated response.",
                        message.attachments.len()
                    )
                } else {
                    "I received your message! This is a simulated response.".to_string()
                };

                updated.messages.push(ChatMessage::assistant(response_text));
            }

            chat_response.set(updated);
        })
    };

    html! {
        <div class="h-screen flex flex-col">
            <div class="bg-blue-600 dark:bg-gray-800 text-white p-4 shadow-lg">
                <div class="flex justify-between items-center">
                    <h1 class="text-2xl font-bold">{"Gate Chat UI Demo"}</h1>
                    <div class="flex items-center gap-4">
                        <select
                            onchange={on_cassette_select}
                            disabled={*loading_cassette}
                            class="px-3 py-1.5 rounded bg-white/10 hover:bg-white/20 transition-colors cursor-pointer text-sm"
                        >
                            <option value="" class="bg-gray-800">{"Load example..."}</option>
                            {
                                available_cassettes.iter().map(|cassette| {
                                    let value = format!("{}:{}", cassette.provider, cassette.name);
                                    let label = format!("{} - {}",
                                        cassette.provider.to_uppercase(),
                                        cassette.display_name
                                    );
                                    html! {
                                        <option value={value} class="bg-gray-800">{label}</option>
                                    }
                                }).collect::<Html>()
                            }
                        </select>
                        <button
                            onclick={toggle_dark_mode}
                            class="p-2 rounded-lg bg-white/10 hover:bg-white/20 transition-colors"
                            title={if *dark_mode { "Switch to light mode" } else { "Switch to dark mode" }}
                        >
                            if *dark_mode {
                                // Sun icon for light mode
                                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-6 h-6">
                                    <path stroke-linecap="round" stroke-linejoin="round" d="M12 3v2.25m6.364.386l-1.591 1.591M21 12h-2.25m-.386 6.364l-1.591-1.591M12 18.75V21m-4.773-4.227l-1.591 1.591M5.25 12H3m4.227-4.773L5.636 5.636M15.75 12a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0z" />
                                </svg>
                            } else {
                                // Moon icon for dark mode
                                <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor" class="w-6 h-6">
                                    <path stroke-linecap="round" stroke-linejoin="round" d="M21.752 15.002A9.718 9.718 0 0118 15.75c-5.385 0-9.75-4.365-9.75-9.75 0-1.33.266-2.597.748-3.752A9.753 9.753 0 003 11.25C3 16.635 7.365 21 12.75 21a9.753 9.753 0 009.002-5.998z" />
                                </svg>
                            }
                        </button>
                    </div>
                </div>
                if let Some(error) = &*cassette_error {
                    <div class="mt-2 text-sm text-red-300">
                        {error}
                    </div>
                }
                if *loading_cassette {
                    <div class="mt-2 text-sm">
                        {"Loading cassette..."}
                    </div>
                }
            </div>
            <div class="flex-1 overflow-hidden">
                <ChatContainer
                    chat_response={(*chat_response).clone()}
                    show_metadata={true}
                    show_input={true}
                    on_send_multimodal={Some(on_send_multimodal)}
                    allow_images={true}
                    allow_files={true}
                    input_disabled={false}
                />
            </div>
        </div>
    }
}

#[wasm_bindgen(start)]
#[allow(clippy::main_recursion)]
pub fn main() {
    console_error_panic_hook::set_once();
    yew::Renderer::<App>::new().render();
}
