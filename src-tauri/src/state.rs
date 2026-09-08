use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, bail};
use tokio::sync::{Mutex, OnceCell, RwLock};

use crate::apple;
use crate::dto::{
    AppStatus, AssuranceInput, GatewayAccess, IdentityStatus, ProviderConfig, RunKind, RunRequest,
    ServiceState, ServiceStatus,
};
use crate::history::History;
use crate::identity;

struct RuntimeStatus {
    provider: ServiceStatus,
    provider_handle: Option<hellas_sdk::ProviderHandle>,
    gateway: ServiceStatus,
    gateway_access: Option<GatewayAccess>,
    gateway_handle: Option<hellas_sdk::gateway::GatewayHandle>,
    gateway_config: Option<RunRequest>,
}

pub struct AppState {
    pub history: History,
    socket_path: PathBuf,
    counter_directory: PathBuf,
    identity: hellas_sdk::ClientIdentity,
    #[cfg(target_os = "macos")]
    provider_identity_path: PathBuf,
    #[cfg(target_os = "macos")]
    provider_state_directory: PathBuf,
    provider_identity: OnceCell<Arc<crate::provider_identity::ProviderIdentity>>,
    client: OnceCell<hellas_sdk::HellasClient>,
    lifecycle: Mutex<()>,
    runtime: RwLock<RuntimeStatus>,
}

impl AppState {
    pub fn open(data_dir: &Path) -> anyhow::Result<Self> {
        let identity = identity::load_or_create(&data_dir.join("identity"))?;
        Ok(Self {
            history: History::open(&data_dir.join("history.sqlite3"))?,
            socket_path: data_dir.join("gate.sock"),
            counter_directory: data_dir.join("apple-assertion-counters"),
            identity,
            #[cfg(target_os = "macos")]
            provider_identity_path: data_dir.join("provider-identity"),
            #[cfg(target_os = "macos")]
            provider_state_directory: data_dir.join("provider"),
            provider_identity: OnceCell::new(),
            client: OnceCell::new(),
            lifecycle: Mutex::new(()),
            runtime: RwLock::new(RuntimeStatus {
                provider: stopped("Not configured"),
                provider_handle: None,
                gateway: stopped("Not configured"),
                gateway_access: None,
                gateway_handle: None,
                gateway_config: None,
            }),
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub async fn status(&self) -> AppStatus {
        let runtime = self.runtime.read().await;
        AppStatus {
            version: env!("CARGO_PKG_VERSION").into(),
            identity: IdentityStatus {
                producer_id: identity::producer_id(&self.identity),
                caller_public_key: identity::public_key(&self.identity),
                node_id: self.identity.node_id().to_string(),
                attestation: if self.provider_identity.get().is_some() {
                    "apple-app-attest".into()
                } else {
                    apple::availability().label.into()
                },
                detail: if self.provider_identity.get().is_some() {
                    "Apple App Attest provider enrollment is active".into()
                } else {
                    apple::availability().detail.into()
                },
            },
            provider: runtime.provider.clone(),
            gateway: runtime.gateway.clone(),
            endpoint: runtime
                .provider_handle
                .as_ref()
                .map(|handle| handle.node_id().to_string())
                .unwrap_or_default(),
            socket_path: self.socket_path.display().to_string(),
        }
    }

    pub async fn set_provider_enabled(
        &self,
        enabled: bool,
        config: Option<ProviderConfig>,
    ) -> anyhow::Result<AppStatus> {
        let _lifecycle = self.lifecycle.lock().await;
        if !enabled {
            let handle = {
                let mut runtime = self.runtime.write().await;
                runtime.provider = ServiceStatus {
                    state: ServiceState::Stopping,
                    detail: "Stopping Hellas Fetch provider".into(),
                };
                runtime.provider_handle.take()
            };
            if let Some(handle) = handle {
                handle.shutdown().await;
            }
            self.runtime.write().await.provider = stopped("Stopped by user");
            return Ok(self.status().await);
        }

        let config = {
            let runtime = self.runtime.read().await;
            if runtime.provider_handle.is_some() {
                drop(runtime);
                return Ok(self.status().await);
            }
            config.context("configure an upstream and allowed caller before serving")?
        };
        self.runtime.write().await.provider = ServiceStatus {
            state: ServiceState::Starting,
            detail: "Creating or loading the attested provider identity".into(),
        };
        match self.start_provider(config).await {
            Ok(handle) => {
                let node_id = handle.node_id().to_string();
                let sockets = handle
                    .bound_sockets()
                    .into_iter()
                    .map(|address| address.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut runtime = self.runtime.write().await;
                runtime.provider = ServiceStatus {
                    state: ServiceState::Running,
                    detail: format!("Attested Fetch provider {node_id} on {sockets}"),
                };
                runtime.provider_handle = Some(handle);
                drop(runtime);
                Ok(self.status().await)
            }
            Err(error) => {
                self.runtime.write().await.provider = ServiceStatus {
                    state: ServiceState::Failed,
                    detail: error.to_string(),
                };
                Err(error)
            }
        }
    }

    #[cfg(target_os = "macos")]
    async fn start_provider(
        &self,
        config: ProviderConfig,
    ) -> anyhow::Result<hellas_sdk::ProviderHandle> {
        anyhow::ensure!(
            !config.service.trim().is_empty() && !config.method.trim().is_empty(),
            "Fetch service and method must be non-empty"
        );
        anyhow::ensure!(
            !config.openai_api_key.trim().is_empty(),
            "OpenAI API key must be non-empty"
        );
        let callers = config
            .allowed_callers
            .iter()
            .map(|key| decode_public_key(key))
            .collect::<anyhow::Result<Vec<_>>>()?;
        anyhow::ensure!(
            !callers.is_empty(),
            "at least one allowed caller is required"
        );
        let provider_identity = self
            .provider_identity
            .get_or_try_init(|| async {
                crate::provider_identity::load_or_create(
                    &self.provider_identity_path,
                    &self.identity,
                )
                .await
                .map(Arc::new)
            })
            .await?;
        hellas_sdk::start_openai_provider(hellas_sdk::OpenAiProviderOptions {
            port: config.port,
            identity: self.identity.clone(),
            enrollment: provider_identity.enrollment().clone(),
            root: provider_identity.root(),
            state_directory: self.provider_state_directory.clone(),
            service: config.service,
            method: config.method,
            bearer_token: config.openai_api_key,
            allowed_callers: callers,
            fetch_max_in_flight: hellas_rpc::DEFAULT_FETCH_MAX_IN_FLIGHT,
            fetch_queue_capacity: hellas_rpc::DEFAULT_FETCH_QUEUE_CAPACITY,
            retained_transcript_capacity: hellas_rpc::DEFAULT_FETCH_RETAINED_TRANSCRIPT_CAPACITY,
            fetch_replay_max_in_flight: hellas_rpc::DEFAULT_FETCH_REPLAY_MAX_IN_FLIGHT,
        })
        .await
    }

    #[cfg(not(target_os = "macos"))]
    async fn start_provider(
        &self,
        _config: ProviderConfig,
    ) -> anyhow::Result<hellas_sdk::ProviderHandle> {
        bail!("the attested provider requires a provisioned macOS Gate build")
    }

    pub async fn set_gateway_enabled(
        &self,
        enabled: bool,
        config: Option<RunRequest>,
    ) -> anyhow::Result<AppStatus> {
        let _lifecycle = self.lifecycle.lock().await;
        if !enabled {
            let handle = {
                let mut runtime = self.runtime.write().await;
                runtime.gateway = ServiceStatus {
                    state: ServiceState::Stopping,
                    detail: "Stopping loopback listener".into(),
                };
                runtime.gateway_access = None;
                runtime.gateway_handle.take()
            };
            if let Some(handle) = handle {
                handle.shutdown().await?;
            }
            self.runtime.write().await.gateway = stopped("Stopped by user");
            return Ok(self.status().await);
        }

        let config = {
            let mut runtime = self.runtime.write().await;
            if runtime.gateway_handle.is_some() {
                drop(runtime);
                return Ok(self.status().await);
            }
            if let Some(config) = config {
                runtime.gateway_config = Some(config);
            }
            runtime
                .gateway_config
                .clone()
                .context("configure a trusted Fetch provider before starting the gateway")?
        };
        let options = self.fetch_gateway_options(&config)?;
        self.runtime.write().await.gateway = ServiceStatus {
            state: ServiceState::Starting,
            detail: "Binding a private loopback endpoint".into(),
        };
        match hellas_sdk::gateway::start_fetch(options).await {
            Ok(handle) => {
                let access = GatewayAccess {
                    address: format!("http://{}", handle.address()),
                    bearer: handle.bearer().to_owned(),
                };
                let mut runtime = self.runtime.write().await;
                runtime.gateway = ServiceStatus {
                    state: ServiceState::Running,
                    detail: format!("Responses endpoint at {}/v1/responses", access.address),
                };
                runtime.gateway_access = Some(access);
                runtime.gateway_handle = Some(handle);
                drop(runtime);
                Ok(self.status().await)
            }
            Err(error) => {
                self.runtime.write().await.gateway = ServiceStatus {
                    state: ServiceState::Failed,
                    detail: error.to_string(),
                };
                Err(error)
            }
        }
    }

    pub async fn gateway_access(&self) -> Option<GatewayAccess> {
        self.runtime.read().await.gateway_access.clone()
    }

    fn fetch_gateway_options(
        &self,
        request: &RunRequest,
    ) -> anyhow::Result<hellas_sdk::gateway::FetchGatewayOptions> {
        if !matches!(request.kind, RunKind::Fetch) {
            bail!("the minimal loopback gateway supports sealed Fetch Responses")
        }
        let (node_id, node_addrs, execution_environment, assurance, provider_trust) =
            self.remote_fetch_parameters(request)?;
        Ok(hellas_sdk::gateway::FetchGatewayOptions {
            host: "127.0.0.1".into(),
            port: Some(0),
            node_id: Some(node_id),
            node_addrs,
            retries: 1,
            service: request.service.clone(),
            method: request.method.clone(),
            execution_environment,
            request_overrides: serde_json::Map::new(),
            provider_trust,
            caller_key: self.identity.caller_key().clone(),
            assurance,
            secret_key: self.identity.transport_key(),
        })
    }

    pub async fn fetch(
        &self,
        request: &RunRequest,
    ) -> anyhow::Result<
        std::pin::Pin<
            Box<
                dyn futures::Stream<
                        Item = hellas_sdk::client::ClientResult<
                            hellas_sdk::client::FetchExecutionEvent,
                        >,
                    > + Send,
            >,
        >,
    > {
        if !matches!(request.kind, RunKind::Fetch) {
            bail!("causal-LM execution is not configured in this build")
        }
        let (node_id, node_addrs, execution_environment, assurance, provider_trust) =
            self.remote_fetch_parameters(request)?;
        let client = self
            .client
            .get_or_try_init(|| hellas_sdk::HellasClient::open_with_identity(self.identity.clone()))
            .await?;
        client
            .fetch(hellas_sdk::RemoteFetchRequest {
                node_id: Some(node_id),
                node_addrs,
                retries: 1,
                service: request.service.clone(),
                method: request.method.clone(),
                execution_environment,
                payload: request.input.as_bytes().to_vec(),
                retention: hellas_rpc::Retention::Ephemeral,
                assurance,
                provider_trust,
            })
            .map_err(Into::into)
    }

    fn remote_fetch_parameters(
        &self,
        request: &RunRequest,
    ) -> anyhow::Result<(
        hellas_sdk::iroh::EndpointId,
        Vec<std::net::SocketAddr>,
        hellas_rpc::ContentId,
        hellas_rpc::Assurance,
        hellas_sdk::client::ProviderTrustAnchor,
    )> {
        let node_id = request
            .target
            .parse()
            .context("invalid provider endpoint ID")?;
        let node_addrs = request
            .node_addresses
            .iter()
            .map(|address| address.parse().context("invalid provider socket address"))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let expected_genesis = request
            .trust_anchor
            .parse()
            .context("invalid provider genesis content ID")?;
        let execution_environment = request
            .execution_environment
            .parse()
            .context("invalid execution-environment content ID")?;
        let assurance = match request.assurance {
            AssuranceInput::ProducerSigned => hellas_rpc::Assurance::ProducerSigned,
            AssuranceInput::AppleAppAttest => hellas_rpc::Assurance::AppleAppAttest,
        };
        let apple_app_attest = if matches!(request.assurance, AssuranceInput::AppleAppAttest) {
            if request.apple_app_id.is_empty() || request.apple_cd_hashes.is_empty() {
                bail!("Apple App Attest requires an app ID and at least one allowed CDHash")
            }
            let hashes = request
                .apple_cd_hashes
                .iter()
                .map(|hash| decode_hash(hash))
                .collect::<anyhow::Result<Vec<_>>>()?;
            Some(hellas_sdk::client::AppleAppAttestTrust::new(
                request.apple_app_id.clone(),
                hashes,
                Arc::new(hellas_sdk::FilesystemAssertionCounterStore::new(
                    &self.counter_directory,
                )),
            ))
        } else {
            None
        };
        let provider_trust = hellas_sdk::client::ProviderTrustAnchor {
            expected_genesis,
            required_assurance: assurance,
            apple_app_attest,
        };
        Ok((
            node_id,
            node_addrs,
            execution_environment,
            assurance,
            provider_trust,
        ))
    }
}

fn stopped(detail: &str) -> ServiceStatus {
    ServiceStatus {
        state: ServiceState::Stopped,
        detail: detail.into(),
    }
}

fn decode_hash(value: &str) -> anyhow::Result<[u8; 32]> {
    if value.len() != 64 {
        bail!("Apple CDHash must contain exactly 64 hexadecimal characters")
    }
    let mut output = [0_u8; 32];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let text = std::str::from_utf8(pair)?;
        output[index] = u8::from_str_radix(text, 16).context("Apple CDHash is not hexadecimal")?;
    }
    Ok(output)
}

#[cfg(target_os = "macos")]
fn decode_public_key(value: &str) -> anyhow::Result<hellas_rpc::PublicKey> {
    if value.len() != 66 {
        bail!("allowed caller public keys must contain 66 hexadecimal characters")
    }
    let mut output = [0_u8; 33];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let text = std::str::from_utf8(pair)?;
        output[index] =
            u8::from_str_radix(text, 16).context("allowed caller public key is not hexadecimal")?;
    }
    Ok(hellas_rpc::PublicKey::Secp256k1(output))
}
