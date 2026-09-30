use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hellas_gateway::{WorkExecutionBackend, WorkFetchRequest};
use hellas_rpc::protocol::{
    work_fetch::FetchRoutePolicy,
    work_grant::{
        UnixMillis,
        records::{Principal, SignedOffer},
    },
    work_profile::WorkPolicy,
};
use hellas_sdk::grant_client::{GrantSessionOptions, GrantTransport, PinnedOffer, UnpinnedOffer};
use hellas_sdk::grant_gateway::GrantGateway;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, OnceCell, RwLock};

use crate::apple;
use crate::dto::{
    AppStatus, AssuranceInput, GatewayAccess, IdentityStatus, ProviderConfig, RunKind, RunRequest,
    ServiceState, ServiceStatus, WorkTarget,
};
use crate::history::History;
use crate::identity;

#[derive(Clone)]
struct ClientTarget {
    backend: Arc<dyn WorkExecutionBackend>,
    provider: hellas_sdk::iroh::EndpointId,
    service: String,
    method: String,
    environment: hellas_rpc::ContentId,
}

struct RuntimeStatus {
    stopping: bool,
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
    chain: crate::chain::ChainNode,
    #[cfg(target_os = "macos")]
    provider_identity_path: PathBuf,
    #[cfg(target_os = "macos")]
    provider_state_directory: PathBuf,
    provider_identity: OnceCell<Arc<crate::provider_identity::ProviderIdentity>>,
    contact: Principal,
    journal_root: PathBuf,
    client: Mutex<Option<(String, ClientTarget)>>,
    lifecycle: Mutex<()>,
    runtime: RwLock<RuntimeStatus>,
}

impl AppState {
    pub fn open(data_dir: &Path) -> anyhow::Result<Self> {
        let identity = identity::load_or_create(&data_dir.join("identity"))?;
        let contact = identity::contact(&data_dir.join("contact-root"), &identity)?;
        Ok(Self {
            history: History::open(&data_dir.join("history.sqlite3"))?,
            socket_path: data_dir.join("gate.sock"),
            counter_directory: data_dir.join("apple-assertion-counters"),
            identity,
            chain: crate::chain::ChainNode::new(data_dir.join("chain")),
            #[cfg(target_os = "macos")]
            provider_identity_path: data_dir.join("provider-identity"),
            #[cfg(target_os = "macos")]
            provider_state_directory: data_dir.join("provider"),
            provider_identity: OnceCell::new(),
            contact,
            journal_root: data_dir.join("work-channels"),
            client: Mutex::new(None),
            lifecycle: Mutex::new(()),
            runtime: RwLock::new(RuntimeStatus {
                stopping: false,
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
                contact: STANDARD.encode(self.contact.bundle().canonical_bytes()),
                contact_id: self.contact.id().0.to_string(),
                apple_app_id: option_env!("HELLAS_GATE_TRUST_APP_ID")
                    .unwrap_or_default()
                    .into(),
                apple_cd_hashes: option_env!("HELLAS_GATE_TRUST_CDHASHES")
                    .unwrap_or_default()
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
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
        anyhow::ensure!(
            !enabled || !self.runtime.read().await.stopping,
            "Gate is shutting down"
        );
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
            config.context("configure an upstream and contacts before serving")?
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
        crate::provider_setup::start(
            config,
            self.identity.clone(),
            provider_identity.enrollment().clone(),
            provider_identity.root(),
            self.provider_state_directory.clone(),
            &self.chain,
        )
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
        anyhow::ensure!(
            !enabled || !self.runtime.read().await.stopping,
            "Gate is shutting down"
        );
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
                self.client.lock().await.take();
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
        let target = self.client_for(&config).await?;
        anyhow::ensure!(
            target.environment == hellas_rpc::FetchEnvironment::OpenAiResponses.manifest_id(),
            "the Responses gateway requires an OpenAI Responses resource"
        );
        let options = hellas_gateway::FetchGatewayOptions {
            host: "127.0.0.1".into(),
            port: Some(0),
            provider: target.provider,
            service: target.service,
            method: target.method,
            request_overrides: Default::default(),
            work: target.backend,
        };
        self.runtime.write().await.gateway = ServiceStatus {
            state: ServiceState::Starting,
            detail: "Binding a private loopback endpoint".into(),
        };
        match hellas_gateway::start_fetch(options).await {
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
                self.client.lock().await.take();
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

    /// The first quit request drains every accepted operation before exit.
    pub async fn shutdown(&self) -> anyhow::Result<bool> {
        let _lifecycle = self.lifecycle.lock().await;
        let (gateway, provider) = {
            let mut runtime = self.runtime.write().await;
            if runtime.stopping {
                return Ok(false);
            }
            runtime.stopping = true;
            (
                runtime.gateway_handle.take(),
                runtime.provider_handle.take(),
            )
        };
        let client = self.client.lock().await.take();
        let result = if let Some(gateway) = gateway {
            gateway.shutdown().await
        } else {
            if let Some((_, client)) = client {
                client.backend.drain().await;
            }
            Ok(())
        };
        if let Some(provider) = provider {
            provider.shutdown().await;
        }
        self.chain.shutdown().await?;
        result?;
        Ok(true)
    }

    pub async fn export_offers(&self) -> anyhow::Result<Vec<crate::dto::ProviderOffer>> {
        self.runtime
            .read()
            .await
            .provider_handle
            .as_ref()
            .context("start the provider before exporting Offers")?
            .offers()?
            .into_iter()
            .map(|offer| {
                Ok(crate::dto::ProviderOffer {
                    contact: offer.offer().grant.kind.principal().id().0.to_string(),
                    offer: STANDARD.encode(offer.encode()?),
                })
            })
            .collect()
    }

    pub async fn fetch(
        &self,
        request: &RunRequest,
    ) -> anyhow::Result<hellas_gateway::WorkOutputStream<hellas_rpc::output::OutputEvent>> {
        let _lifecycle = self.lifecycle.lock().await;
        let target = self.client_for(request).await?;
        Ok(target.backend.fetch(WorkFetchRequest {
            provider: target.provider,
            service: target.service,
            method: target.method,
            body: request.input.as_bytes().to_vec(),
        })?)
    }

    async fn client_for(&self, request: &RunRequest) -> anyhow::Result<ClientTarget> {
        anyhow::ensure!(!self.runtime.read().await.stopping, "Gate is shutting down");
        anyhow::ensure!(
            matches!(request.kind, RunKind::Fetch),
            "select a Fetch target"
        );
        let key = serde_json::to_string(&(
            &request.target,
            &request.assurance,
            &request.apple_app_id,
            &request.apple_cd_hashes,
        ))?;
        let mut client = self.client.lock().await;
        if let Some((old, target)) = client.as_ref()
            && old == &key
        {
            return Ok(target.clone());
        }
        anyhow::ensure!(
            self.runtime.read().await.gateway_handle.is_none(),
            "stop the gateway before switching target or trust policy"
        );
        if let Some((_, previous)) = client.take() {
            previous.backend.drain().await;
        }
        let target = match &request.target {
            WorkTarget::Authorized {
                offer: record,
                resource,
            } => {
                let offer = self.pinned_offer(request, record)?;
                SignedOffer::decode(
                    &offer.signed().encode()?,
                    self.contact.id(),
                    UnixMillis(u64::try_from(
                        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
                    )?),
                )?;
                let policy = offer
                    .offer()
                    .grant
                    .policies
                    .iter()
                    .find(|p| p.name == *resource)
                    .context("Offer does not name this resource")?;
                let (service, method, environment) = fetch_route(&policy.work)?;
                let provider = hellas_sdk::iroh::EndpointId::from_bytes(
                    &offer.offer().provider.grant_transport()?,
                )?;
                let endpoint =
                    hellas_sdk::iroh::Endpoint::builder(hellas_sdk::iroh::endpoint::presets::N0)
                        .secret_key(self.identity.transport_key())
                        .bind()
                        .await?;
                let opened = GrantGateway::open(
                    GrantSessionOptions {
                        target: offer.clone(),
                        client: self.contact.clone(),
                        signer: Arc::new(self.identity.caller_key().clone()),
                        journal_root: self.journal_root.clone(),
                        timeout: Duration::from_millis(offer.offer().grant.max_job_millis.get()),
                    },
                    GrantTransport::Remote(endpoint.clone()),
                    Some(resource.clone()),
                )
                .await;
                let backend = match opened {
                    Ok(backend) => backend,
                    Err(error) => {
                        endpoint.close().await;
                        return Err(error.into());
                    }
                };
                ClientTarget {
                    backend,
                    provider,
                    service,
                    method,
                    environment,
                }
            }
            WorkTarget::Paid {
                pool_config,
                provider,
                route,
            } => {
                let options = hellas_sdk::paid_gateway::load_pool_options(
                    Path::new(pool_config),
                    assurance(request.assurance),
                )?;
                let provider: hellas_sdk::iroh::EndpointId = provider.parse()?;
                let entry = options
                    .providers
                    .iter()
                    .find(|p| p.provider == provider)
                    .context("pool does not name this provider")?;
                let (service, method, environment) =
                    paid_route(&entry.config.work_policy, route.as_ref())?;
                let node = self.chain.get(&entry.config).await?;
                let backend = hellas_sdk::paid_gateway::PaidGateway::open(
                    options,
                    self.identity.clone(),
                    node,
                )
                .await?;
                ClientTarget {
                    backend,
                    provider,
                    service,
                    method,
                    environment,
                }
            }
        };
        *client = Some((key, target.clone()));
        Ok(target)
    }

    pub async fn provision_paid_offer(&self, path: &Path, preview: bool) -> anyhow::Result<String> {
        let _lifecycle = self.lifecycle.lock().await;
        anyhow::ensure!(!self.runtime.read().await.stopping, "Gate is shutting down");
        anyhow::ensure!(
            self.runtime.read().await.provider_handle.is_none(),
            "stop the provider before provisioning a bond"
        );
        #[cfg(target_os = "macos")]
        let provider = self
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
        #[cfg(not(target_os = "macos"))]
        let provider = self
            .provider_identity
            .get()
            .context("the attested provider requires a provisioned macOS Gate build")?;
        crate::paid::provision(
            path,
            &self.identity,
            provider.enrollment().clone(),
            preview,
            &self.chain,
        )
        .await
    }

    fn pinned_offer(&self, request: &RunRequest, record: &str) -> anyhow::Result<PinnedOffer> {
        let bytes = decode_record(
            record,
            hellas_rpc::protocol::work_grant::records::MAX_OFFER_BYTES,
        )?;
        let now = UnixMillis(0); // A running session refreshes standing independently of bootstrap expiry.
        let signed = SignedOffer::decode(&bytes, self.contact.id(), now)?;
        let expected_genesis = signed.offer().provider.content_id();
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
        Ok(UnpinnedOffer::decode(&bytes, self.contact.id(), now)?.pin(&provider_trust)?)
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

pub(crate) fn decode_record(record: &str, max: usize) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        record.len() <= max.div_ceil(3) * 4,
        "imported record exceeds size limit"
    );
    STANDARD
        .decode(record.trim())
        .context("record must be base64 exported by Hellas")
}

fn fetch_route(policy: &WorkPolicy) -> anyhow::Result<(String, String, hellas_rpc::ContentId)> {
    match policy {
        WorkPolicy::Fetch {
            policy,
            route: FetchRoutePolicy::SealedRoute { service, method },
        } => Ok((service.clone(), method.clone(), policy.allowed_environment)),
        _ => bail!("target requires a sealed Fetch resource"),
    }
}

fn assurance(value: AssuranceInput) -> hellas_rpc::Assurance {
    match value {
        AssuranceInput::ProducerSigned => hellas_rpc::Assurance::ProducerSigned,
        AssuranceInput::AppleAppAttest => hellas_rpc::Assurance::AppleAppAttest,
    }
}

fn paid_route(
    policy: &WorkPolicy,
    selected: Option<&crate::dto::FetchRouteInput>,
) -> anyhow::Result<(String, String, hellas_rpc::ContentId)> {
    let route = match policy {
        WorkPolicy::Fetch {
            policy,
            route: FetchRoutePolicy::OpenFetch { .. },
        } => {
            let selected =
                selected.context("select the provider's HTTPS Fetch service and method")?;
            FetchRoutePolicy::sealed_route(&selected.service, &selected.method)?;
            (
                selected.service.clone(),
                selected.method.clone(),
                policy.allowed_environment,
            )
        }
        _ => fetch_route(policy)?,
    };
    if let Some(selected) = selected {
        anyhow::ensure!(
            selected.service == route.0 && selected.method == route.1,
            "selected route differs from the paid policy"
        );
    }
    Ok(route)
}
