pub mod bootstrap;
pub mod context;
pub mod errors;
pub mod inference;
pub mod prelude;
pub mod state;
pub mod types;
pub mod validation;
pub mod webauthn;

#[cfg(feature = "tracing")]
pub mod tracing;

#[cfg(any(test, feature = "tests"))]
pub mod tests;

pub use bootstrap::BootstrapTokenValidator;
pub use context::RequestContext;
pub use errors::{Error, Result};
pub use inference::InferenceBackend;
pub use state::StateBackend;
pub use validation::{ValidateConfig, validators};
pub use webauthn::{StoredCredential, WebAuthnBackend};

// Re-export types for convenience
pub use types::{
    ApiKey, ChatCompletionRequest, ChatCompletionResponse, Error as ProtoError, HookAction,
    HookResponse, MessagesRequest, MessagesResponse, Model, ModelType, Organization, Provider,
    ProviderType, RequestHookContext, ResponseHookContext, StreamingChatCompletionResponse,
    StreamingMessagesResponse, TimeRange, UsageRecord, User,
};
