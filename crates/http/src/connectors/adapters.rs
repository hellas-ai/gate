use super::http_connector::Provider;
use gate_core::router::protocols::ProtocolAdapter;
use gate_core::router::types::Protocol;
use http::HeaderMap;

pub trait HttpEndpointAdapter: ProtocolAdapter {
    fn endpoint_path(&self, provider: &Provider) -> Option<&'static str>;
}

pub struct OpenAIChatAdapter;
impl ProtocolAdapter for OpenAIChatAdapter {
    fn protocol(&self) -> Protocol {
        Protocol::OpenAIChat
    }
    fn is_streaming_response(&self, headers: &HeaderMap) -> bool {
        headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|ct| ct.contains("text/event-stream"))
            .unwrap_or(false)
    }
}

impl HttpEndpointAdapter for OpenAIChatAdapter {
    fn endpoint_path(&self, provider: &Provider) -> Option<&'static str> {
        match provider {
            Provider::OpenAI => Some("/v1/chat/completions"),
            // For Anthropic, SDK maps Chat to Messages
            Provider::Anthropic => Some("/v1/messages"),
            _ => None,
        }
    }
}

pub struct OpenAIResponsesAdapter;
impl ProtocolAdapter for OpenAIResponsesAdapter {
    fn protocol(&self) -> Protocol {
        Protocol::OpenAIResponses
    }
    fn is_streaming_response(&self, _headers: &HeaderMap) -> bool {
        true
    }
}

impl HttpEndpointAdapter for OpenAIResponsesAdapter {
    fn endpoint_path(&self, provider: &Provider) -> Option<&'static str> {
        match provider {
            Provider::OpenAI => Some("/v1/responses"),
            Provider::OpenAICodex => Some("/responses"),
            _ => None,
        }
    }
}

pub struct AnthropicMessagesAdapter;
impl ProtocolAdapter for AnthropicMessagesAdapter {
    fn protocol(&self) -> Protocol {
        Protocol::Anthropic
    }
    fn is_streaming_response(&self, headers: &HeaderMap) -> bool {
        headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|ct| ct.contains("text/event-stream"))
            .unwrap_or(false)
    }
}

impl HttpEndpointAdapter for AnthropicMessagesAdapter {
    fn endpoint_path(&self, provider: &Provider) -> Option<&'static str> {
        match provider {
            Provider::Anthropic => Some("/v1/messages"),
            _ => None,
        }
    }
}

pub struct OpenAIMessagesAdapter;
impl ProtocolAdapter for OpenAIMessagesAdapter {
    fn protocol(&self) -> Protocol {
        Protocol::OpenAIMessages
    }
    fn is_streaming_response(&self, headers: &HeaderMap) -> bool {
        headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|ct| ct.contains("text/event-stream"))
            .unwrap_or(false)
    }
}

impl HttpEndpointAdapter for OpenAIMessagesAdapter {
    fn endpoint_path(&self, provider: &Provider) -> Option<&'static str> {
        match provider {
            Provider::OpenAI => Some("/v1/messages"),
            _ => None,
        }
    }
}
