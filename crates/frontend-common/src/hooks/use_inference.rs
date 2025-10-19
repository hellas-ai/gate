//! Hook for accessing the inference service with authentication

use crate::auth::context::use_auth_client;
use crate::services::InferenceService;
use yew::prelude::*;

/// Hook to get an inference service with authentication
#[hook]
pub fn use_inference() -> Option<InferenceService> {
    let client = use_auth_client()?;
    Some(InferenceService::new(client))
}
