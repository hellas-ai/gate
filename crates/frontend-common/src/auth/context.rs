//! Global authentication context and provider

use crate::client::get_base_url;
use crate::components::ReauthModal;
use crate::config::AuthConfig;
use gate_http::client::{AuthenticatedGateClient, PublicGateClient};
use gloo::timers::callback::Timeout;
use std::cell::RefCell;
use std::rc::Rc;
use web_sys::Storage;
use yew::prelude::*;

/// Substates for public client scenarios
#[derive(Clone, Debug, PartialEq)]
pub enum PublicAuthStatus {
    /// Just not authenticated
    Unauthenticated { error: Option<String> },

    /// Auth invalid/expired
    Invalid {
        reason: InvalidReason,
        previous_user: Option<String>,
        show_modal: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum InvalidReason {
    Expired,
    TokenInvalid,
    Revoked,
}

/// Authentication state with clear client type separation
#[derive(Clone)]
pub enum AuthState {
    /// Still determining auth state
    Loading,

    /// Has public client - various unauthenticated states
    Public {
        client: PublicGateClient,
        status: PublicAuthStatus,
    },

    /// Has authenticated client - logged in
    Authenticated {
        client: AuthenticatedGateClient,
        user_id: String,
        name: String,
        token: String,
        expires_at: Option<i64>,
    },
}

/// Authentication context data
#[derive(Clone)]
pub struct AuthContextData {
    pub state: AuthState,
}

impl PartialEq for AuthContextData {
    fn eq(&self, other: &Self) -> bool {
        // Compare based on the state variant and key fields, not the clients themselves
        match (&self.state, &other.state) {
            (AuthState::Loading, AuthState::Loading) => true,
            (
                AuthState::Public {
                    status: status1, ..
                },
                AuthState::Public {
                    status: status2, ..
                },
            ) => status1 == status2,
            (
                AuthState::Authenticated {
                    user_id: id1,
                    token: token1,
                    ..
                },
                AuthState::Authenticated {
                    user_id: id2,
                    token: token2,
                    ..
                },
            ) => id1 == id2 && token1 == token2,
            _ => false,
        }
    }
}

/// Authentication context actions
pub enum AuthAction {
    Initialize,
    Login {
        user_id: String,
        name: String,
        token: String,
    },
    Logout,
    SessionExpired {
        previous_user: String,
    },
    ValidateToken,
    ShowReauthModal,
    HideReauthModal,
    SetError(String),
}

/// Authentication context
pub type AuthContext = UseReducerHandle<AuthContextData>;

impl Default for AuthContextData {
    fn default() -> Self {
        Self {
            state: AuthState::Loading,
        }
    }
}

impl AuthContextData {
    /// Get a public client (available in public states, or via downgrade)
    pub fn public_client(&self) -> Option<PublicGateClient> {
        match &self.state {
            AuthState::Public { client, .. } => Some(client.clone()),
            AuthState::Authenticated { client, .. } => Some(client.to_public()),
            AuthState::Loading => None,
        }
    }

    /// Get authenticated client (only when authenticated)
    pub fn auth_client(&self) -> Option<AuthenticatedGateClient> {
        match &self.state {
            AuthState::Authenticated { client, .. } => Some(client.clone()),
            _ => None,
        }
    }
}

impl Reducible for AuthContextData {
    type Action = AuthAction;

    fn reduce(self: Rc<Self>, action: Self::Action) -> Rc<Self> {
        match action {
            AuthAction::Initialize => {
                // Create initial public client
                let client = match PublicGateClient::new(get_base_url()) {
                    Ok(client) => client,
                    Err(e) => {
                        gloo::console::error!("Failed to create public client:", e.to_string());
                        return self;
                    }
                };

                Rc::new(Self {
                    state: AuthState::Public {
                        client,
                        status: PublicAuthStatus::Unauthenticated { error: None },
                    },
                })
            }
            AuthAction::Login {
                user_id,
                name,
                token,
            } => {
                // Create authenticated client
                let client = match AuthenticatedGateClient::new(get_base_url(), &token) {
                    Ok(client) => client,
                    Err(e) => {
                        gloo::console::error!(
                            "Failed to create authenticated client:",
                            e.to_string()
                        );
                        return self;
                    }
                };

                // Save to sessionStorage
                if let Some(storage) = get_session_storage() {
                    let auth_data = serde_json::json!({
                        "user_id": user_id,
                        "name": name,
                        "token": token,
                    });
                    if let Ok(serialized) = serde_json::to_string(&auth_data) {
                        let _ = storage.set_item(AuthConfig::AUTH_STATE_KEY, &serialized);
                    }
                }

                Rc::new(Self {
                    state: AuthState::Authenticated {
                        client,
                        user_id,
                        name,
                        token,
                        expires_at: None,
                    },
                })
            }
            AuthAction::Logout => {
                // Clear from sessionStorage
                if let Some(storage) = get_session_storage() {
                    let _ = storage.remove_item(AuthConfig::AUTH_STATE_KEY);
                }

                // Create new public client
                let client = match PublicGateClient::new(get_base_url()) {
                    Ok(client) => client,
                    Err(e) => {
                        gloo::console::error!("Failed to create public client:", e.to_string());
                        return self;
                    }
                };

                Rc::new(Self {
                    state: AuthState::Public {
                        client,
                        status: PublicAuthStatus::Unauthenticated { error: None },
                    },
                })
            }
            AuthAction::SessionExpired { previous_user } => {
                // Clear from sessionStorage
                if let Some(storage) = get_session_storage() {
                    let _ = storage.remove_item(AuthConfig::AUTH_STATE_KEY);
                }

                // Create new public client
                let client = match PublicGateClient::new(get_base_url()) {
                    Ok(client) => client,
                    Err(e) => {
                        gloo::console::error!("Failed to create public client:", e.to_string());
                        return self;
                    }
                };

                Rc::new(Self {
                    state: AuthState::Public {
                        client,
                        status: PublicAuthStatus::Invalid {
                            reason: InvalidReason::Expired,
                            previous_user: Some(previous_user),
                            show_modal: true,
                        },
                    },
                })
            }
            AuthAction::ValidateToken => {
                // Check if token is still valid for authenticated state
                if let AuthState::Authenticated {
                    expires_at: Some(expires_at),
                    user_id,
                    ..
                } = &self.state
                {
                    let now = js_sys::Date::now() as i64 / 1000;
                    if now >= *expires_at {
                        // Token expired - create action for session expired
                        let user_id = user_id.clone();
                        return Rc::new(self.as_ref().clone()).reduce(AuthAction::SessionExpired {
                            previous_user: user_id,
                        });
                    }
                }
                self
            }
            AuthAction::ShowReauthModal => {
                // Update status to show modal if in Invalid state
                match &self.state {
                    AuthState::Public { client, status } => {
                        if let PublicAuthStatus::Invalid {
                            reason,
                            previous_user,
                            ..
                        } = status
                        {
                            Rc::new(Self {
                                state: AuthState::Public {
                                    client: client.clone(),
                                    status: PublicAuthStatus::Invalid {
                                        reason: reason.clone(),
                                        previous_user: previous_user.clone(),
                                        show_modal: true,
                                    },
                                },
                            })
                        } else {
                            self
                        }
                    }
                    _ => self,
                }
            }
            AuthAction::HideReauthModal => {
                // Hide modal if in Invalid state
                match &self.state {
                    AuthState::Public { client, status } => {
                        if let PublicAuthStatus::Invalid {
                            reason,
                            previous_user,
                            ..
                        } = status
                        {
                            Rc::new(Self {
                                state: AuthState::Public {
                                    client: client.clone(),
                                    status: PublicAuthStatus::Invalid {
                                        reason: reason.clone(),
                                        previous_user: previous_user.clone(),
                                        show_modal: false,
                                    },
                                },
                            })
                        } else {
                            self
                        }
                    }
                    _ => self,
                }
            }
            AuthAction::SetError(error) => {
                // Set error in public state
                match &self.state {
                    AuthState::Public { client, .. } => Rc::new(Self {
                        state: AuthState::Public {
                            client: client.clone(),
                            status: PublicAuthStatus::Unauthenticated { error: Some(error) },
                        },
                    }),
                    _ => self,
                }
            }
        }
    }
}

/// Get sessionStorage
fn get_session_storage() -> Option<Storage> {
    web_sys::window().and_then(|w| w.session_storage().ok().flatten())
}

/// Auth provider props
#[derive(Properties, PartialEq)]
pub struct AuthProviderProps {
    pub children: Children,
}

/// Auth provider component
#[function_component(AuthProvider)]
pub fn auth_provider(props: &AuthProviderProps) -> Html {
    let auth_state = use_reducer(AuthContextData::default);

    // Set up global auth error handler
    {
        let auth_state = auth_state.clone();
        use_effect_with((), move |_| {
            let auth_state = auth_state.clone();
            super::error_handler::set_auth_error_callback(Rc::new(move || {
                auth_state.dispatch(AuthAction::ShowReauthModal);
            }));

            // Cleanup on unmount
            move || {
                super::error_handler::clear_auth_error_callback();
            }
        });
    }

    // Initialize and load auth state from sessionStorage on mount
    {
        let auth_state = auth_state.clone();
        use_effect_with((), move |_| {
            // First initialize with a public client
            auth_state.dispatch(AuthAction::Initialize);

            // Then check for stored auth
            if let Some(storage) = get_session_storage() {
                if let Ok(Some(stored)) = storage.get_item(AuthConfig::AUTH_STATE_KEY) {
                    if let Ok(data) = serde_json::from_str::<serde_json::Value>(&stored) {
                        // Extract fields from the stored data
                        if let (Some(user_id), Some(name), Some(token)) = (
                            data.get("user_id").and_then(|v| v.as_str()),
                            data.get("name").and_then(|v| v.as_str()),
                            data.get("token").and_then(|v| v.as_str()),
                        ) {
                            // Check expiration if present
                            if let Some(expires_at) =
                                data.get("expires_at").and_then(|v| v.as_i64())
                            {
                                let now = js_sys::Date::now() as i64 / 1000;
                                if now >= expires_at {
                                    // Token expired
                                    auth_state.dispatch(AuthAction::SessionExpired {
                                        previous_user: user_id.to_string(),
                                    });
                                    return;
                                }
                            }

                            // Valid token, login
                            auth_state.dispatch(AuthAction::Login {
                                user_id: user_id.to_string(),
                                name: name.to_string(),
                                token: token.to_string(),
                            });
                        }
                    }
                }
            }
        });
    }

    // Set up periodic token validation
    {
        let auth_state = auth_state.clone();
        let is_authenticated = matches!(auth_state.state, AuthState::Authenticated { .. });
        use_effect_with(is_authenticated, move |is_auth| {
            let cleanup: Box<dyn FnOnce()> = if *is_auth {
                // Check token every minute for authenticated state
                let auth_state = auth_state.clone();
                let handle = Timeout::new(AuthConfig::TOKEN_REFRESH_INTERVAL_MS, move || {
                    auth_state.dispatch(AuthAction::ValidateToken);
                });

                // Store handle in a RefCell to access in cleanup
                let handle = Rc::new(RefCell::new(Some(handle)));
                let handle_clone = handle.clone();

                // Return cleanup function
                Box::new(move || {
                    if let Some(h) = handle_clone.borrow_mut().take() {
                        h.forget();
                    }
                })
            } else {
                // Return empty cleanup
                Box::new(|| {})
            };
            cleanup
        });
    }

    html! {
        <ContextProvider<AuthContext> context={auth_state}>
            <ReauthModal />
            {props.children.clone()}
        </ContextProvider<AuthContext>>
    }
}

/// Hook to use auth context
#[hook]
pub fn use_auth() -> AuthContext {
    use_context::<AuthContext>()
        .expect("AuthContext not found. Make sure to wrap your component with AuthProvider")
}

/// Hook to get public client
#[hook]
pub fn use_public_client() -> Option<PublicGateClient> {
    let auth = use_auth();
    auth.public_client()
}

/// Hook to get authenticated client
#[hook]
pub fn use_auth_client() -> Option<AuthenticatedGateClient> {
    let auth = use_auth();
    auth.auth_client()
}

/// Hook to check if authenticated
#[hook]
pub fn use_is_authenticated() -> bool {
    let auth = use_auth();
    matches!(auth.state, AuthState::Authenticated { .. })
}
