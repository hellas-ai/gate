use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Debug, Clone)]
pub struct ProviderMetadata {
    pub id: &'static str,
    pub display_name: &'static str,
    pub icon_path: &'static str,
    pub default_base_url: &'static str,
    pub requires_api_key: bool,
    pub supported_models: Vec<&'static str>,
    pub default_headers: Vec<(&'static str, &'static str)>,
    pub placeholder_api_key: &'static str,
}

pub static PROVIDER_REGISTRY: LazyLock<HashMap<&'static str, ProviderMetadata>> =
    LazyLock::new(|| {
        let providers = vec![
            ProviderMetadata {
                id: "openai",
                display_name: "OpenAI",
                icon_path: "/assets/providers/openai.svg",
                default_base_url: "https://api.openai.com",
                requires_api_key: true,
                supported_models: vec![
                    "gpt-4",
                    "gpt-4-turbo",
                    "gpt-3.5-turbo",
                    "text-embedding-ada-002",
                ],
                default_headers: vec![],
                placeholder_api_key: "sk-...",
            },
            ProviderMetadata {
                id: "anthropic",
                display_name: "Anthropic",
                icon_path: "/assets/providers/anthropic.svg",
                default_base_url: "https://api.anthropic.com",
                requires_api_key: true,
                supported_models: vec![
                    "claude-3-opus-20240229",
                    "claude-3-sonnet-20240229",
                    "claude-3-haiku-20240307",
                    "claude-2.1",
                ],
                default_headers: vec![("anthropic-version", "2023-06-01")],
                placeholder_api_key: "sk-ant-...",
            },
            ProviderMetadata {
                id: "groq",
                display_name: "Groq",
                icon_path: "/assets/providers/groq.svg",
                default_base_url: "https://api.groq.com/openai",
                requires_api_key: true,
                supported_models: vec!["llama2-70b-4096", "mixtral-8x7b-32768", "gemma-7b-it"],
                default_headers: vec![],
                placeholder_api_key: "gsk_...",
            },
            ProviderMetadata {
                id: "mistral",
                display_name: "Mistral AI",
                icon_path: "/assets/providers/mistral.svg",
                default_base_url: "https://api.mistral.ai",
                requires_api_key: true,
                supported_models: vec![
                    "mistral-large-latest",
                    "mistral-medium-latest",
                    "mistral-small-latest",
                    "mistral-embed",
                ],
                default_headers: vec![],
                placeholder_api_key: "...",
            },
            ProviderMetadata {
                id: "gemini",
                display_name: "Google Gemini",
                icon_path: "/assets/providers/gemini.svg",
                default_base_url: "https://generativelanguage.googleapis.com",
                requires_api_key: true,
                supported_models: vec!["gemini-pro", "gemini-pro-vision"],
                default_headers: vec![],
                placeholder_api_key: "AIza...",
            },
            ProviderMetadata {
                id: "cohere",
                display_name: "Cohere",
                icon_path: "/assets/providers/cohere.svg",
                default_base_url: "https://api.cohere.ai",
                requires_api_key: true,
                supported_models: vec![
                    "command",
                    "command-nightly",
                    "command-light",
                    "embed-english-v3.0",
                ],
                default_headers: vec![],
                placeholder_api_key: "...",
            },
            ProviderMetadata {
                id: "perplexity",
                display_name: "Perplexity",
                icon_path: "/assets/providers/perplexity.svg",
                default_base_url: "https://api.perplexity.ai",
                requires_api_key: true,
                supported_models: vec![
                    "pplx-7b-online",
                    "pplx-70b-online",
                    "codellama-34b-instruct",
                ],
                default_headers: vec![],
                placeholder_api_key: "pplx-...",
            },
            ProviderMetadata {
                id: "custom",
                display_name: "Custom Provider",
                icon_path: "/assets/providers/custom.svg",
                default_base_url: "https://api.example.com",
                requires_api_key: false,
                supported_models: vec![],
                default_headers: vec![],
                placeholder_api_key: "your-api-key",
            },
        ];

        providers.into_iter().map(|p| (p.id, p)).collect()
    });

pub fn get_all_providers() -> Vec<&'static ProviderMetadata> {
    let mut providers: Vec<_> = PROVIDER_REGISTRY.values().collect();
    // Sort by display name, but keep "Custom" at the end
    providers.sort_by(|a, b| {
        if a.id == "custom" {
            std::cmp::Ordering::Greater
        } else if b.id == "custom" {
            std::cmp::Ordering::Less
        } else {
            a.display_name.cmp(b.display_name)
        }
    });
    providers
}

impl ProviderMetadata {
    pub fn to_default_config(&self, name: String) -> super::super::types::ProviderConfig {
        super::super::types::ProviderConfig {
            name,
            provider: self.id.to_string(),
            base_url: self.default_base_url.to_string(),
            api_key: None,
            timeout_seconds: 30,
            models: self
                .supported_models
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}
