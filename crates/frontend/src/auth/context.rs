//! Global authentication context and provider

use crate::client::set_auth_token;
use crate::config::AuthConfig;
use gloo::timers::callback::Timeout;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use web_sys::Storage;
use yew::prelude::*;

/// Authentication state
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthState {
    pub user_id: String,
    pub name: String,
    pub token: String,
    pub role: String,
    pub expires_at: Option<i64>, // Unix timestamp
}

/// Authentication context data
#[derive(Clone, Debug, PartialEq)]
pub struct AuthContextData {
    pub auth_state: Option<AuthState>,
    pub is_loading: bool,
    pub error: Option<String>,
}

/// Authentication context actions
pub enum AuthAction {
    Login(AuthState),
    Logout,
    SetLoading(bool),
    ValidateToken,
}

/// Authentication context
pub type AuthContext = UseReducerHandle<AuthContextData>;

impl Default for AuthContextData {
    fn default() -> Self {
        Self {
            auth_state: None,
            is_loading: true, // Start with loading to check sessionStorage
            error: None,
        }
    }
}

impl Reducible for AuthContextData {
    type Action = AuthAction;

    fn reduce(self: Rc<Self>, action: Self::Action) -> Rc<Self> {
        match action {
            AuthAction::Login(auth_state) => {
                // Update the client with the auth token
                let _ = set_auth_token(Some(&auth_state.token));

                // Save to sessionStorage
                if let Some(storage) = get_session_storage()
                    && let Ok(serialized) = serde_json::to_string(&auth_state)
                {
                    let _ = storage.set_item(AuthConfig::AUTH_STATE_KEY, &serialized);
                }

                Rc::new(Self {
                    auth_state: Some(auth_state),
                    is_loading: false,
                    error: None,
                })
            }
            AuthAction::Logout => {
                // Clear the auth token from client - this creates a fresh unauthenticated client
                let _ = set_auth_token(None);

                // Clear from sessionStorage
                if let Some(storage) = get_session_storage() {
                    let _ = storage.remove_item(AuthConfig::AUTH_STATE_KEY);
                }

                Rc::new(Self {
                    auth_state: None,
                    is_loading: false,
                    error: None,
                })
            }
            AuthAction::SetLoading(is_loading) => Rc::new(Self {
                is_loading,
                ..(*self).clone()
            }),
            AuthAction::ValidateToken => {
                // Check if token is still valid
                if let Some(auth_state) = &self.auth_state
                    && let Some(expires_at) = auth_state.expires_at
                {
                    let now = js_sys::Date::now() as i64 / 1000;
                    if now >= expires_at {
                        // Token expired
                        if let Some(storage) = get_session_storage() {
                            let _ = storage.remove_item(AuthConfig::AUTH_STATE_KEY);
                        }
                        return Rc::new(Self {
                            auth_state: None,
                            is_loading: false,
                            error: Some("Session expired. Please login again.".to_string()),
                        });
                    }
                }
                Rc::new(self.as_ref().clone())
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

    // Load auth state from sessionStorage on mount
    {
        let auth_state = auth_state.clone();
        use_effect_with((), move |_| {
            if let Some(storage) = get_session_storage()
                && let Ok(Some(stored)) = storage.get_item(AuthConfig::AUTH_STATE_KEY)
                && let Ok(state) = serde_json::from_str::<AuthState>(&stored)
            {
                // Validate token expiration
                if let Some(expires_at) = state.expires_at {
                    let now = js_sys::Date::now() as i64 / 1000;
                    if now < expires_at {
                        auth_state.dispatch(AuthAction::Login(state));
                        return;
                    }
                } else {
                    // No expiration, consider valid
                    auth_state.dispatch(AuthAction::Login(state));
                    return;
                }
            }
            // No valid auth found
            auth_state.dispatch(AuthAction::SetLoading(false));
        });
    }

    // Set up periodic token validation
    {
        let auth_state = auth_state.clone();
        use_effect_with(auth_state.auth_state.clone(), move |current_auth| {
            let cleanup: Box<dyn FnOnce()> = if current_auth.is_some() {
                // Check token every minute
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

/// Hook to get current auth state
#[hook]
pub fn use_auth_state() -> Option<AuthState> {
    let auth = use_auth();
    auth.auth_state.clone()
}

/// Hook to check if authenticated
#[hook]
pub fn use_is_authenticated() -> bool {
    let auth = use_auth();
    auth.auth_state.is_some()
}
