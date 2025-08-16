pub mod access;
pub mod context;
pub mod errors;
pub mod inference;
pub mod state;
pub mod types;

#[cfg(feature = "tracing")]
#[macro_use]
extern crate tracing as tracing_crate;

#[cfg(feature = "tracing")]
#[macro_use]
pub mod tracing;

#[cfg(any(test, feature = "tests"))]
pub mod tests;

pub use context::RequestContext;
pub use errors::{Error, Result};
pub use inference::InferenceBackend;
pub use state::StateBackend;

// Re-export types for convenience
pub use types::{
    ApiKey, ChatCompletionRequest, ChatCompletionResponse, Error as ProtoError, HookAction,
    HookResponse, MessagesRequest, MessagesResponse, Model, ModelType, Organization, Provider,
    ProviderType, RequestHookContext, ResponseHookContext, StreamingChatCompletionResponse,
    StreamingMessagesResponse, TimeRange, UsageRecord, User,
};
