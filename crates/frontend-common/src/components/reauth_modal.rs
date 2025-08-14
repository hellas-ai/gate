//! Re-authentication modal component

use crate::auth::{use_auth, AuthAction};
use crate::hooks::{use_webauthn, WebAuthnState};
use yew::prelude::*;

/// Re-authentication modal that appears when session expires
#[function_component(ReauthModal)]
pub fn reauth_modal() -> Html {
    let auth = use_auth();
    let webauthn = use_webauthn();
    let is_authenticating = use_state(|| false);

    let on_reauth = {
        let webauthn = webauthn.clone();
        let is_authenticating = is_authenticating.clone();

        Callback::from(move |e: MouseEvent| {
            e.prevent_default();
            is_authenticating.set(true);
            webauthn.authenticate();
        })
    };

    // Monitor WebAuthn state changes
    {
        let is_authenticating = is_authenticating.clone();

        use_effect_with(webauthn.state().clone(), move |state| {
            match state {
                WebAuthnState::Idle => {
                    is_authenticating.set(false);
                }
                WebAuthnState::Processing => {
                    // Keep showing loading state
                }
                WebAuthnState::Error(_) => {
                    is_authenticating.set(false);
                }
            }
        });
    }

    // Auto-hide modal when authenticated
    {
        let auth = auth.clone();
        use_effect_with(auth.auth_state.clone(), move |auth_state| {
            if auth_state.is_some() && auth.show_reauth_modal {
                auth.dispatch(AuthAction::HideReauthModal);
            }
        });
    }

    // Only render modal content if show_reauth_modal is true
    if !auth.show_reauth_modal {
        return html! {};
    }

    html! {
        <div class="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
            <div class="bg-white dark:bg-gray-800 rounded-lg p-6 max-w-md w-full mx-4 shadow-xl">
                <div class="flex items-center mb-4">
                    <svg class="w-8 h-8 text-yellow-500 mr-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
                            d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
                    </svg>
                    <h2 class="text-xl font-bold text-gray-900 dark:text-white">
                        {"Session Expired"}
                    </h2>
                </div>

                <p class="text-gray-600 dark:text-gray-300 mb-6">
                    {"Your session has expired. Please re-authenticate to continue using the application."}
                </p>

                if let Some(error) = &auth.error {
                    <div class="mb-4 p-3 bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-300 rounded text-sm">
                        {error}
                    </div>
                }

                if let WebAuthnState::Error(error) = webauthn.state() {
                    <div class="mb-4 p-3 bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-300 rounded text-sm">
                        {error}
                    </div>
                }

                <button
                    onclick={on_reauth}
                    disabled={*is_authenticating}
                    class="w-full bg-blue-600 hover:bg-blue-700 disabled:bg-gray-400
                           text-white font-medium py-3 px-4 rounded-lg transition-colors
                           flex items-center justify-center"
                >
                    if *is_authenticating {
                        <svg class="animate-spin h-5 w-5 mr-2" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                        </svg>
                        {"Authenticating..."}
                    } else {
                        <svg class="w-5 h-5 mr-2" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
                                d="M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z" />
                        </svg>
                        {"Re-authenticate"}
                    }
                </button>
            </div>
        </div>
    }
}
