use gate_frontend_common::{
    auth::use_auth,
    components::Spinner as LoadingSpinner,
    hooks::{use_webauthn, WebAuthnState},
};
use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct OnboardingAuthProps {
    pub bootstrap_token: String,
}

#[function_component(OnboardingAuth)]
pub fn onboarding_auth(props: &OnboardingAuthProps) -> Html {
    let webauthn = use_webauthn();
    let auth = use_auth();
    let name = use_state(String::new);
    let bootstrap_token = props.bootstrap_token.clone();

    // Redirect to home if user is authenticated
    use_effect_with(auth.auth_state.clone(), {
        move |auth_state| {
            if auth_state.is_some() {
                // User is authenticated, redirect to home
                let window = web_sys::window().unwrap();
                let location = window.location();
                location.set_href("/").ok();
            }
        }
    });

    // Handle input changes
    let on_name_input = {
        let name = name.clone();
        Callback::from(move |e: InputEvent| {
            let input: web_sys::HtmlInputElement = e.target_unchecked_into();
            name.set(input.value());
        })
    };

    // Handle registration with bootstrap token
    let on_register = {
        let webauthn = webauthn.clone();
        let name = name.clone();
        let bootstrap_token = bootstrap_token.clone();
        Callback::from(move |_| {
            let name_value = (*name).clone();
            web_sys::console::log_1(&format!("on_register called, name: '{name_value}'").into());
            if !name_value.is_empty() {
                web_sys::console::log_1(&"Calling webauthn.register".into());
                webauthn.register(name_value, None, Some(bootstrap_token.clone()));
            }
        })
    };

    // Main onboarding UI
    html! {
        <div class="min-h-screen flex items-center justify-center bg-gradient-to-br from-gray-900 via-blue-900 to-purple-900">
            <div class="max-w-md w-full p-8">
                <div class="backdrop-blur-lg bg-white/10 rounded-2xl shadow-2xl p-8 border border-white/20">
                    <div class="text-center mb-8">
                        <h1 class="text-3xl font-bold text-white mb-2">{"Welcome to Gate"}</h1>
                        <p class="text-white/70">{"Let's set up your first admin account"}</p>
                    </div>

                    {match webauthn.state() {
                        WebAuthnState::Processing => html! {
                            <div class="text-center">
                                <LoadingSpinner text={Some("Creating your account...".to_string())} />
                                <p class="text-sm text-white/70 mt-4">
                                    {"Use your device's biometrics or security key"}
                                </p>
                            </div>
                        },
                        WebAuthnState::Error(error) => html! {
                            <div class="space-y-4">
                                <div class="bg-red-500/20 border border-red-500/30 rounded-lg p-4 text-center">
                                    <p class="text-red-200 text-sm">{error}</p>
                                </div>
                                {registration_form(&name, &on_name_input, &on_register)}
                            </div>
                        },
                        WebAuthnState::Idle => html! {
                            {registration_form(&name, &on_name_input, &on_register)}
                        }
                    }}

                    <div class="mt-6 text-center">
                        <p class="text-white/50 text-xs">
                            {"This is a one-time setup. Your credentials will be securely stored."}
                        </p>
                    </div>
                </div>
            </div>
        </div>
    }
}

fn registration_form(
    name: &UseStateHandle<String>,
    on_name_input: &Callback<InputEvent>,
    on_register: &Callback<MouseEvent>,
) -> Html {
    // Simple keyboard handler directly on the div
    let handle_keydown = {
        let name = name.clone();
        let on_register = on_register.clone();
        Callback::from(move |e: KeyboardEvent| {
            web_sys::console::log_1(
                &format!("Keydown event: key='{}', code='{}'", e.key(), e.code()).into(),
            );
            if e.key() == "Enter" && !(*name).is_empty() {
                web_sys::console::log_1(
                    &"Enter pressed with non-empty name, triggering register".into(),
                );
                e.prevent_default();
                // Just emit a dummy mouse event to trigger the registration
                let event = web_sys::MouseEvent::new("click").unwrap();
                on_register.emit(event);
            }
        })
    };

    html! {
        <div class="space-y-4">
            <div>
                <label class="block text-white/80 text-sm font-medium mb-2">
                    {"Your Name"}
                </label>
                <input
                    type="text"
                    class="w-full px-4 py-3 bg-white/10 border border-white/20 rounded-lg text-white placeholder-white/50 focus:outline-none focus:border-blue-400 focus:bg-white/20 transition-all"
                    placeholder="Enter your name"
                    value={(**name).clone()}
                    oninput={on_name_input}
                    onkeydown={handle_keydown}
                />
                <p class="text-white/50 text-xs mt-2">
                    {"This will be displayed when you log in"}
                </p>
            </div>

            <button
                class="w-full px-4 py-3 bg-gradient-to-r from-blue-500 to-purple-600 hover:from-blue-600 hover:to-purple-700 text-white rounded-lg font-medium transition-all disabled:opacity-50 disabled:cursor-not-allowed"
                onclick={on_register}
                disabled={(**name).is_empty()}
            >
                {"Create Admin Account"}
            </button>
        </div>
    }
}
