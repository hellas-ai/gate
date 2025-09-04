use crate::Settings;
use crate::daemon::Daemon;
use gate_core::Result;
use gate_core::router::index::SinkIndex;
use gate_core::router::middleware::KeyCaptureRegistrar;
use gate_core::router::registry::SinkRegistry;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct DaemonKeyRegistrar {
    daemon: Daemon,
    sink_registry: Arc<SinkRegistry>,
    sink_index: Arc<SinkIndex>,
    created: Mutex<HashSet<String>>, // prevent duplicate work in-process
}

impl DaemonKeyRegistrar {
    pub fn new(
        daemon: Daemon,
        sink_registry: Arc<SinkRegistry>,
        sink_index: Arc<SinkIndex>,
    ) -> Self {
        Self {
            daemon,
            sink_registry,
            sink_index,
            created: Mutex::new(HashSet::new()),
        }
    }

    async fn settings_has_anthropic_key(settings: &Settings, key: &str) -> bool {
        settings.providers.iter().any(|p| {
            matches!(p.provider, crate::config::ProviderType::Anthropic)
                && p.api_key.as_deref() == Some(key)
        })
    }
}

#[async_trait::async_trait]
impl KeyCaptureRegistrar for DaemonKeyRegistrar {
    async fn register_anthropic_key(&self, key: &str) -> Result<()> {
        // Dedup per-process
        {
            let mut guard = self.created.lock().await;
            if !guard.insert(key.to_string()) {
                return Ok(());
            }
        }

        // Check current settings
        let settings = match self.daemon.get_settings().await {
            Ok(s) => s,
            Err(_) => return Ok(()),
        };
        if Self::settings_has_anthropic_key(&settings, key).await {
            return Ok(());
        }

        // Build a new provider config entry
        let mut new_settings = settings.clone();
        // Choose a unique name
        let base_name = "anthropic";
        let mut name = base_name.to_string();
        let mut i = 1u32;
        while new_settings.providers.iter().any(|p| p.name == name) {
            name = format!("{base_name}-{i}");
            i += 1;
        }

        let provider_cfg = crate::config::ProviderConfig {
            name: name.clone(),
            provider: crate::config::ProviderType::Anthropic,
            base_url: "https://api.anthropic.com".to_string(),
            api_key: Some(key.to_string()),
            timeout_seconds: 600,
            models: vec![],
        };
        new_settings.providers.push(provider_cfg);

        // Persist settings with system identity
        let _ = self
            .daemon
            .system_identity()
            .update_config(new_settings)
            .await;

        // Register sink immediately, and drop fallback if present
        let sink = match gate_http::sinks::anthropic::create_sink(
            gate_http::sinks::anthropic::AnthropicConfig {
                api_key: Some(key.to_string()),
                base_url: None,
                timeout_seconds: None,
                sink_id: Some(format!("provider://anthropic/{name}")),
            },
        )
        .await
        {
            Ok(s) => Arc::new(s),
            Err(_) => return Ok(()), // Don't fail request path on registration issues
        };

        // Remove fallback if present to avoid routing ambiguity
        self.sink_registry
            .remove("provider://anthropic/fallback")
            .await;
        self.sink_registry
            .register(format!("provider://anthropic/{name}"), sink)
            .await;

        // Refresh index
        let _ = self
            .sink_index
            .refresh_from_registry(&self.sink_registry)
            .await;

        Ok(())
    }
}
