//! WebAuthn hook for simplified authentication flows

use crate::auth::error_messages::get_user_friendly_error;
use crate::auth::{context::use_public_client, use_auth, AuthAction};
use crate::services::{AuthApiService, WebAuthnBrowserService};
use yew::prelude::*;

/// WebAuthn operation state
#[derive(Clone, Debug, PartialEq)]
pub enum WebAuthnState {
    Idle,
    Processing,
    Error(String),
}

/// WebAuthn hook handle
#[derive(Clone)]
pub struct UseWebAuthnHandle {
    auth_api: Option<AuthApiService>,
    browser_service: WebAuthnBrowserService,
    auth_context: UseReducerHandle<crate::auth::context::AuthContextData>,
    state: UseStateHandle<WebAuthnState>,
}

impl UseWebAuthnHandle {
    /// Register a new device
    pub fn register(
        &self,
        name: String,
        device_name: Option<String>,
        bootstrap_token: Option<String>,
    ) {
        let auth_api = self.auth_api.clone();
        let browser_service = self.browser_service.clone();
        let auth_context = self.auth_context.clone();
        let state = self.state.clone();

        wasm_bindgen_futures::spawn_local(async move {
            state.set(WebAuthnState::Processing);

            // Check WebAuthn support
            if !browser_service.is_supported() {
                state.set(WebAuthnState::Error(
                    "WebAuthn is not supported in this browser".to_string(),
                ));
                return;
            }

            // Get the auth API
            let Some(api) = auth_api else {
                state.set(WebAuthnState::Error(
                    "Authentication service not available".to_string(),
                ));
                return;
            };

            // Start registration
            match api.start_registration(&name).await {
                Ok(start_response) => {
                    gloo::console::log!("Registration started, creating credential...");

                    // Create credential
                    match browser_service
                        .create_credential(start_response.challenge)
                        .await
                    {
                        Ok(credential) => {
                            gloo::console::log!("Credential created, completing registration...");

                            // Complete registration
                            match api
                                .complete_registration(
                                    start_response.session_id,
                                    credential,
                                    device_name,
                                    bootstrap_token,
                                )
                                .await
                            {
                                Ok(complete_response) => {
                                    gloo::console::log!("Registration successful!");

                                    // Update auth state
                                    auth_context.dispatch(AuthAction::Login {
                                        user_id: complete_response.user_id,
                                        name: complete_response.name,
                                        token: complete_response.token,
                                    });

                                    state.set(WebAuthnState::Idle);
                                }
                                Err(e) => {
                                    gloo::console::error!("Registration failed:", &e.to_string());
                                    state.set(WebAuthnState::Error(get_user_friendly_error(&e)));
                                }
                            }
                        }
                        Err(e) => {
                            gloo::console::error!("Credential creation failed:", &e.to_string());
                            // e is a WebAuthnBrowserError, not ClientError
                            state.set(WebAuthnState::Error(e.to_string()));
                        }
                    }
                }
                Err(e) => {
                    gloo::console::error!("Registration start failed:", &e.to_string());
                    state.set(WebAuthnState::Error(get_user_friendly_error(&e)));
                }
            }
        });
    }

    /// Authenticate with an existing device
    pub fn authenticate(&self) {
        let auth_api = self.auth_api.clone();
        let browser_service = self.browser_service.clone();
        let auth_context = self.auth_context.clone();
        let state = self.state.clone();

        wasm_bindgen_futures::spawn_local(async move {
            state.set(WebAuthnState::Processing);

            // Check WebAuthn support
            if !browser_service.is_supported() {
                state.set(WebAuthnState::Error(
                    "WebAuthn is not supported in this browser".to_string(),
                ));
                return;
            }

            // Get the auth API
            let Some(api) = auth_api else {
                state.set(WebAuthnState::Error(
                    "Authentication service not available".to_string(),
                ));
                return;
            };

            // Start authentication
            match api.start_authentication().await {
                Ok(start_response) => {
                    gloo::console::log!("Authentication started, getting credential...");

                    // Get credential
                    match browser_service
                        .get_credential(start_response.challenge)
                        .await
                    {
                        Ok(credential) => {
                            gloo::console::log!(
                                "Credential obtained, completing authentication..."
                            );

                            // Complete authentication
                            match api
                                .complete_authentication(start_response.session_id, credential)
                                .await
                            {
                                Ok(complete_response) => {
                                    gloo::console::log!("Authentication successful!");

                                    // Update auth state
                                    auth_context.dispatch(AuthAction::Login {
                                        user_id: complete_response.user_id,
                                        name: complete_response.name,
                                        token: complete_response.token,
                                    });

                                    state.set(WebAuthnState::Idle);
                                }
                                Err(e) => {
                                    gloo::console::error!("Authentication failed:", &e.to_string());
                                    state.set(WebAuthnState::Error(get_user_friendly_error(&e)));
                                }
                            }
                        }
                        Err(e) => {
                            gloo::console::error!("Credential get failed:", &e.to_string());
                            // e is a WebAuthnBrowserError, not ClientError
                            state.set(WebAuthnState::Error(e.to_string()));
                        }
                    }
                }
                Err(e) => {
                    gloo::console::error!("Authentication start failed:", &e.to_string());
                    state.set(WebAuthnState::Error(get_user_friendly_error(&e)));
                }
            }
        });
    }

    /// Get the current state
    pub fn state(&self) -> &WebAuthnState {
        &self.state
    }

    /// Clear any error state
    pub fn clear_error(&self) {
        if matches!(*self.state, WebAuthnState::Error(_)) {
            self.state.set(WebAuthnState::Idle);
        }
    }
}

/// Hook to use WebAuthn authentication
#[hook]
pub fn use_webauthn() -> UseWebAuthnHandle {
    let client = use_public_client();
    let auth_api = client.map(AuthApiService::new);
    let browser_service = use_memo((), |_| WebAuthnBrowserService::new());
    let auth_context = use_auth();
    let state = use_state(|| WebAuthnState::Idle);

    UseWebAuthnHandle {
        auth_api,
        browser_service: (*browser_service).clone(),
        auth_context,
        state,
    }
}
