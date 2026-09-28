//! Portable provider assembly, typechecked on every host platform.
use crate::{dto::ProviderConfig, state::decode_public_key};
use anyhow::Context as _;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub async fn start<R: hellas_sdk::RootProver + Send + Sync + 'static>(
    config: ProviderConfig,
    identity: hellas_sdk::ClientIdentity,
    enrollment: hellas_rpc::ProviderEnrollmentBundle,
    root: Arc<R>,
    state_directory: PathBuf,
) -> anyhow::Result<hellas_sdk::ProviderHandle> {
    anyhow::ensure!(
        !config.service.trim().is_empty() && !config.method.trim().is_empty(),
        "Fetch service and method must be non-empty"
    );

    let callers = config
        .allowed_callers
        .iter()
        .map(|key| decode_public_key(key))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let paid_work = config
        .work_config_path
        .as_deref()
        .map(|path| hellas_sdk::work_config::load_work_config(Path::new(path)))
        .transpose()?;
    let mut routes = hellas_sdk::FetchRouteRegistry::new();
    let route = if let Some(http) = config.http_config {
        let config: hellas_sdk::HttpProviderConfig =
            serde_json::from_str(&http).context("invalid HTTPS account configuration")?;
        config.into_entry(hellas_sdk::FetchRoutePolicy::default())?
    } else {
        anyhow::ensure!(
            !config.openai_api_key.trim().is_empty(),
            "OpenAI API key must be non-empty"
        );
        hellas_sdk::FetchRouteEntry::new(
            Arc::new(hellas_sdk::OpenAiResponsesFetchProvider::with_bearer(
                config.openai_api_key,
            )?),
            Arc::new(hellas_sdk::ResponsesFetchAdaptorFactory::new(
                hellas_rpc::FetchEnvironment::OpenAiResponses,
            )),
            hellas_sdk::FetchRoutePolicy::default(),
        )?
    };
    routes.register(
        hellas_sdk::FetchRoute::new(config.service, config.method),
        route,
    )?;
    Ok(
        hellas_sdk::start_fetch_provider(hellas_sdk::FetchProviderOptions {
            port: config.port,
            identity,
            enrollment,
            root,
            state_directory,
            routes,
            paid_work,
            allowed_callers: callers,
            fetch_max_in_flight: hellas_rpc::DEFAULT_FETCH_MAX_IN_FLIGHT,
            fetch_queue_capacity: hellas_rpc::DEFAULT_FETCH_QUEUE_CAPACITY,
            // Gate providers never retain customer request or response bodies.
            // A Retain request must fail before its running marker is written.
            retained_transcript_capacity: 0,
            fetch_replay_max_in_flight: hellas_rpc::DEFAULT_FETCH_REPLAY_MAX_IN_FLIGHT,
        })
        .await?,
    )
}
