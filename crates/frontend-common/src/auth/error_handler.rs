//! Global auth error handler
//!
//! This module provides a global mechanism for handling authentication errors
//! without requiring components to explicitly check for them.

use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    /// Global auth error callback
    static AUTH_ERROR_CALLBACK: RefCell<Option<Rc<dyn Fn()>>> = RefCell::new(None);
}

/// Set the global auth error callback
pub fn set_auth_error_callback(callback: Rc<dyn Fn()>) {
    AUTH_ERROR_CALLBACK.with(|cb| {
        *cb.borrow_mut() = Some(callback);
    });
}

/// Clear the auth error callback
pub fn clear_auth_error_callback() {
    AUTH_ERROR_CALLBACK.with(|cb| {
        *cb.borrow_mut() = None;
    });
}

/// Trigger the auth error callback
pub fn trigger_auth_error() {
    AUTH_ERROR_CALLBACK.with(|cb| {
        if let Some(callback) = cb.borrow().as_ref() {
            callback();
        }
    });
}
