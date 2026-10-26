//! Run and gateway clients consume the same approved SDK backends and funding.
use crate::{contacts, control::LocalNode, dto::*, identity, paid, trust, vault};
use anyhow::{Context, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use hellas_auth::authority::*;
use hellas_gateway::{WorkExecutionBackend, WorkFetchRequest};
use hellas_sdk::{
    grant_client::{GrantSessionOptions, GrantTransport, UnpinnedOffer},
    grant_gateway::GrantGateway,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Mutex;

pub struct Target {
    pub backend: Arc<RetirableBackend>,
    pub provider: hellas_sdk::iroh::EndpointId,
    pub service: String,
    pub method: String,
    pub presentation: FetchPresentation,
}
enum Selection {
    Authorized {
        access: contacts::Imported,
        offer: Box<hellas_sdk::work::model::grant::records::SignedOffer>,
        policy: Box<hellas_sdk::work::model::grant::records::GrantPolicy>,
        trust: Box<trust::Snapshot>,
    },
    Paid {
        pool: PaidPoolInput,
        selected: Box<PaidProviderInput>,
        options: hellas_sdk::paid_gateway::PaidGatewayOptions,
        approvals: Vec<hellas_sdk::trust_policy::ApprovalRecord>,
    },
}
fn route(
    policy: &hellas_sdk::work::model::profile::WorkPolicy,
) -> anyhow::Result<(String, String, FetchPresentation)> {
    let hellas_sdk::work::model::profile::WorkPolicy::Fetch {
        route: hellas_sdk::work::model::fetch::FetchRoutePolicy::SealedRoute { service, method },
        contract,
        ..
    } = policy
    else {
        anyhow::bail!("select a named Fetch resource");
    };
    Ok((
        service.clone(),
        method.clone(),
        if contract.schema() == hellas_rpc::FetchSchema::StatelessResponsesV1 {
            FetchPresentation::Responses
        } else {
            FetchPresentation::Http
        },
    ))
}
impl Selection {
    fn presentation(&self) -> anyhow::Result<FetchPresentation> {
        match self {
            Self::Authorized { policy, .. } => Ok(route(&policy.work)?.2),
            Self::Paid { selected, .. } => Ok(selected.policy.presentation),
        }
    }
}
struct CachedTarget {
    version: String,
    target: Arc<Target>,
}

// Callers retain this proxy, never a journal owner. Retirement removes the owner,
// drains its accepted work, and drops its exclusive journal before any reopen.
pub struct RetirableBackend(std::sync::Mutex<Option<Arc<dyn WorkExecutionBackend>>>);
impl RetirableBackend {
    fn new(backend: Arc<dyn WorkExecutionBackend>) -> Arc<Self> {
        Arc::new(Self(std::sync::Mutex::new(Some(backend))))
    }
    fn with_backend<R>(
        &self,
        use_backend: impl FnOnce(&dyn WorkExecutionBackend) -> R,
    ) -> Result<R, hellas_gateway::WorkGatewayError> {
        let held = self.0.lock().map_err(|_| hellas_gateway::WorkGatewayBusy)?;
        let backend = held.as_ref().ok_or_else(|| {
            hellas_gateway::WorkGatewayError::Rejected(Box::new(std::io::Error::other(
                "Client settings changed; retry the request or restart the gateway listener",
            )))
        })?;
        Ok(use_backend(backend.as_ref()))
    }
    fn is_active(&self) -> bool {
        self.0.lock().is_ok_and(|held| held.is_some())
    }
    async fn retire(&self) -> Result<(), hellas_gateway::WorkShutdownError> {
        let backend = self
            .0
            .lock()
            .map_err(|_| {
                hellas_gateway::WorkShutdownError::Recovery(Arc::new(std::io::Error::other(
                    "client lifecycle lock failed",
                )))
            })?
            .take();
        if let Some(backend) = backend {
            backend.drain().await?;
        }
        Ok(())
    }
}
impl WorkExecutionBackend for RetirableBackend {
    fn fetch_providers(&self) -> Vec<hellas_sdk::iroh::EndpointId> {
        self.with_backend(|backend| backend.fetch_providers())
            .unwrap_or_default()
    }
    fn fetch(
        &self,
        request: hellas_gateway::WorkFetchRequest,
    ) -> Result<
        hellas_gateway::WorkOutputStream<hellas_rpc::output::OutputEvent>,
        hellas_gateway::WorkGatewayError,
    > {
        self.with_backend(|backend| backend.fetch(request))?
    }
    fn execute(
        &self,
        request: hellas_gateway::WorkExecutionRequest,
    ) -> Result<
        hellas_gateway::WorkOutputStream<hellas_gateway::ExecutionEvent>,
        hellas_gateway::WorkGatewayError,
    > {
        self.with_backend(|backend| backend.execute(request))?
    }
    fn timeout(&self) -> Duration {
        self.with_backend(|backend| backend.timeout())
            .unwrap_or_default()
    }
    fn drain(
        &self,
    ) -> futures::future::BoxFuture<'_, Result<(), hellas_gateway::WorkShutdownError>> {
        Box::pin(self.retire())
    }
}
pub struct Clients {
    directory: PathBuf,
    chain: Arc<crate::chain::ChainNode>,
    closed: AtomicBool,
    transport_keys: Mutex<()>,
    #[cfg(test)]
    pub(crate) drained: tokio::sync::Notify,
    #[cfg(test)]
    pub(crate) after_paid_resolution:
        std::sync::Mutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
    #[cfg(test)]
    pub(crate) opened_paid_trust:
        std::sync::Mutex<Vec<Vec<hellas_sdk::client::ProviderTrustAnchor>>>,
    cache: Mutex<BTreeMap<String, CachedTarget>>,
    paid: Mutex<BTreeMap<String, (String, Arc<RetirableBackend>)>>,
}
impl Clients {
    pub fn new(directory: &Path, chain: Arc<crate::chain::ChainNode>) -> Self {
        Self {
            directory: directory.to_owned(),
            chain,
            closed: AtomicBool::new(false),
            transport_keys: Mutex::new(()),
            #[cfg(test)]
            drained: tokio::sync::Notify::new(),
            #[cfg(test)]
            after_paid_resolution: Default::default(),
            #[cfg(test)]
            opened_paid_trust: Default::default(),
            cache: Mutex::new(BTreeMap::new()),
            paid: Mutex::new(BTreeMap::new()),
        }
    }
    pub async fn paid_peer(&self, local: &LocalNode, pool_id: &str) -> anyhow::Result<String> {
        paid::pool_path(&self.directory, pool_id)?;
        ensure!(
            !self.closed.load(Ordering::SeqCst),
            "client admission is closed because Gate is shutting down"
        );
        Ok(self
            .transport(local, &format!("paid:{pool_id}"))
            .await?
            .public()
            .to_string())
    }
    fn selection(&self, local: &LocalNode, input: &WorkClientConfig) -> anyhow::Result<Selection> {
        match &input.target {
            WorkTarget::Authorized {
                access_id,
                allowance,
                resource,
            } => {
                let access = contacts::imported(&self.directory, access_id)?;
                ensure!(
                    access.subject == local.identity.public.owner
                        && access.holder
                            == NodeKey::from_public_key(
                                local.identity.node.node_key().public_key()
                            )?,
                    "imported access belongs to another person or device"
                );
                ensure!(
                    access.expires > UnixMillis::now()?,
                    "imported access expired"
                );
                let offer = access
                    .offers
                    .iter()
                    .map(|s| {
                        Ok(
                            hellas_sdk::work::model::grant::records::SignedOffer::decode(
                                &STANDARD.decode(s)?,
                                UnixMillis::now()?,
                            )?,
                        )
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?
                    .into_iter()
                    .find(|o| o.offer().grant.id.to_string() == *allowance)
                    .context("allowance is absent from this access bundle")?;
                let selected = offer
                    .offer()
                    .grant
                    .policies
                    .iter()
                    .find(|p| p.name == *resource)
                    .context("selected resource is absent from this allowance")?
                    .clone();
                route(&selected.work)?;
                let trust = trust::snapshot(
                    &self.directory,
                    &input.trust_policy,
                    selected.work.allowed_environment(),
                )?;
                Ok(Selection::Authorized {
                    access,
                    offer: Box::new(offer),
                    policy: Box::new(selected),
                    trust: Box::new(trust),
                })
            }
            WorkTarget::Paid {
                pool_id,
                provider,
                service,
                method,
            } => {
                let pool: PaidPoolInput = vault::read(&paid::pool_path(&self.directory, pool_id)?)?
                    .context("configure a typed paid pool first")?;
                let paid::PoolSnapshot { options, approvals } =
                    paid::pool_snapshot(&self.directory, &pool)?;
                let selected = pool
                    .providers
                    .iter()
                    .find(|p| p.provider == *provider)
                    .context("provider is absent from paid pool")?
                    .clone();
                ensure!(
                    selected.trust_policy == input.trust_policy,
                    "Run trust policy must match selected pool provider"
                );
                ensure!(
                    *service == selected.policy.service && *method == selected.policy.method,
                    "paid route differs from selected policy"
                );
                let _: hellas_sdk::iroh::EndpointId = provider.parse()?;
                Ok(Selection::Paid {
                    pool,
                    selected: Box::new(selected),
                    options,
                    approvals,
                })
            }
        }
    }
    pub(crate) fn presentation(
        &self,
        local: &LocalNode,
        input: &WorkClientConfig,
    ) -> anyhow::Result<FetchPresentation> {
        self.selection(local, input)?.presentation()
    }
    pub async fn target(
        &self,
        local: &LocalNode,
        input: &WorkClientConfig,
    ) -> anyhow::Result<Arc<Target>> {
        self.target_for(local, input, None).await
    }
    pub(crate) async fn target_for(
        &self,
        local: &LocalNode,
        input: &WorkClientConfig,
        required: Option<FetchPresentation>,
    ) -> anyhow::Result<Arc<Target>> {
        let selection = self.selection(local, input)?;
        #[cfg(test)]
        if matches!(&selection, Selection::Paid { .. }) {
            let pause = { self.after_paid_resolution.lock().unwrap().take() };
            if let Some((resolved, resume)) = pause {
                resolved.notify_one();
                resume.notified().await;
            }
        }
        if let Some(required) = required {
            ensure!(
                selection.presentation()? == required,
                "selected execution contract differs from required presentation"
            );
        }
        let (policy, pool_version) = match &selection {
            Selection::Authorized { trust, .. } => (&trust.record, String::new()),
            Selection::Paid {
                pool, approvals, ..
            } => {
                let policy = approvals
                    .iter()
                    .find(|record| record.id == input.trust_policy)
                    .context("selected approval is absent from paid snapshot")?;
                (policy, serde_json::to_string(&(pool, approvals))?)
            }
        };
        let version = serde_json::to_string(&(input, policy, pool_version.clone()))?;
        let key = match &input.target {
            WorkTarget::Authorized {
                access_id,
                allowance,
                ..
            } => {
                let access = contacts::imported(&self.directory, access_id)?;
                format!("authorized:{}:{allowance}", access.authority)
            }
            WorkTarget::Paid { .. } => format!("paid:{}", serde_json::to_string(&input.target)?),
        };
        let mut cache = self.cache.lock().await;
        ensure!(
            !self.closed.load(Ordering::SeqCst),
            "client admission is closed because Gate is shutting down"
        );
        if let Some(held) = cache.get(&key)
            && held.version == version
            && held.target.backend.is_active()
        {
            return Ok(held.target.clone());
        }
        let target = match &input.target {
            WorkTarget::Authorized {
                access_id: _,
                allowance,
                resource,
            } => {
                let Selection::Authorized {
                    access,
                    offer,
                    policy: selected,
                    trust,
                } = selection
                else {
                    anyhow::bail!("authorized selection required");
                };
                let (service, method, presentation) = route(&selected.work)?;
                let approved = trust.anchor;
                let provider = hellas_sdk::iroh::EndpointId::from_bytes(
                    &hellas_sdk::work::model::grant::records::grant_transport(
                        &offer.offer().provider,
                    )?,
                )?;
                let pinned =
                    UnpinnedOffer::decode(&offer.encode()?, UnixMillis::now()?)?.pin(&approved)?;
                let credential =
                    identity::decode_credential(&local.vault.required(&access.vault_key()).await?)?;
                let application = Arc::new(hellas_sdk::node_control::profile::ClientProfile {
                    authority: access.authority,
                    authorization: access.authorization,
                    credential,
                    local: None,
                });
                // Refused candidate inputs leave the existing journal owner usable.
                if let Some(held) = cache.remove(&key) {
                    held.target.backend.retire().await?;
                }
                // Outgoing work has its own SDK-owned transport lifetime.
                let transport = GrantTransport::remote(self.transport(local, &key).await?).await?;
                let backend = GrantGateway::open(
                    GrantSessionOptions {
                        target: pinned,
                        principal: access.subject,
                        signer: Arc::new(local.identity.node.node_key().clone()),
                        application,
                        journal_root: self
                            .directory
                            .join("work-channels")
                            .join(access.authority.to_string())
                            .join(allowance),
                        timeout: Duration::from_secs(300),
                    },
                    transport,
                    Some(resource.clone()),
                )
                .await?;
                Target {
                    backend: RetirableBackend::new(backend),
                    provider,
                    service,
                    method,
                    presentation,
                }
            }
            WorkTarget::Paid {
                pool_id,
                provider,
                service,
                method,
            } => {
                let Selection::Paid {
                    pool: _,
                    selected,
                    options,
                    approvals: _,
                } = selection
                else {
                    anyhow::bail!("paid selection required");
                };
                let provider_id: hellas_sdk::iroh::EndpointId = provider.parse()?;
                let first = options.providers.first().context("paid pool is empty")?;
                let chain = self.chain.get(&first.config).await?;
                for provider in &options.providers {
                    provider.config.check_node(&chain)?;
                }
                // Selection, all provider options and chain compatibility are checked
                // before retiring any backend shared by this pool's callers.
                if let Some(held) = cache.remove(&key) {
                    held.target.backend.retire().await?;
                }
                let mut paid_cache = self.paid.lock().await;
                if paid_cache
                    .get(pool_id)
                    .is_some_and(|(held, backend)| held != &pool_version || !backend.is_active())
                    && let Some((_, backend)) = paid_cache.remove(pool_id)
                {
                    backend.retire().await?;
                }
                let backend = if let Some((_, backend)) = paid_cache.get(pool_id) {
                    backend.clone()
                } else {
                    let transport = self.transport(local, &format!("paid:{pool_id}")).await?;
                    let identity = hellas_sdk::NodeIdentity::from_secret_bytes(
                        transport.to_bytes(),
                        local.identity.node.node_secret_bytes(),
                    )?;
                    #[cfg(test)]
                    self.opened_paid_trust.lock().unwrap().push(
                        options
                            .providers
                            .iter()
                            .map(|provider| provider.provider_trust.clone())
                            .collect(),
                    );
                    let backend = hellas_sdk::paid_gateway::PaidGateway::open(
                        options,
                        identity,
                        chain,
                        local.coordinator.clone(),
                    )
                    .await?;
                    let backend = RetirableBackend::new(backend);
                    paid_cache.insert(pool_id.clone(), (pool_version, backend.clone()));
                    backend
                };
                Target {
                    backend,
                    provider: provider_id,
                    service: service.clone(),
                    method: method.clone(),
                    presentation: selected.policy.presentation,
                }
            }
        };
        let target = Arc::new(target);
        cache.insert(
            key,
            CachedTarget {
                version,
                target: target.clone(),
            },
        );
        Ok(target)
    }
    pub async fn fetch(
        &self,
        local: &LocalNode,
        input: &RunRequest,
    ) -> anyhow::Result<hellas_gateway::WorkOutputStream<hellas_rpc::output::OutputEvent>> {
        let target = self.target(local, &input.client).await?;
        Ok(target.backend.fetch(WorkFetchRequest {
            provider: target.provider,
            service: target.service.clone(),
            method: target.method.clone(),
            body: input.input.as_bytes().to_vec(),
        })?)
    }
    async fn transport(
        &self,
        local: &LocalNode,
        id: &str,
    ) -> anyhow::Result<hellas_sdk::iroh::SecretKey> {
        let _keys = self.transport_keys.lock().await;
        ensure!(
            !self.closed.load(Ordering::SeqCst),
            "client admission is closed because Gate is shutting down"
        );
        let key = format!(
            "outgoing-{}",
            hex::encode(&hellas_auth::Digest::hash(id.as_bytes()).as_bytes()[..16])
        );
        let bytes = match local.vault.get(&key).await? {
            Some(bytes) => bytes,
            None => {
                let key_bytes = crate::identity::random::<32>()?;
                local.vault.set(&key, &key_bytes).await?;
                zeroize::Zeroizing::new(key_bytes.to_vec())
            }
        };
        Ok(hellas_sdk::iroh::SecretKey::from_bytes(
            &bytes.as_slice().try_into()?,
        ))
    }
    #[cfg(test)]
    pub(crate) async fn cached_targets(&self) -> usize {
        self.cache.lock().await.len()
    }
    pub async fn reset(&self) -> anyhow::Result<()> {
        let mut cache = self.cache.lock().await;
        let held = std::mem::take(&mut *cache);
        let mut failure = None;
        for target in held.values() {
            if let Err(e) = target.target.backend.retire().await {
                failure.get_or_insert(anyhow::Error::from(e));
            }
        }
        let mut paid = self.paid.lock().await;
        for (_, backend) in paid.values() {
            if let Err(error) = backend.retire().await {
                failure.get_or_insert(anyhow::Error::from(error));
            }
        }
        paid.clear();
        if let Some(e) = failure {
            return Err(e);
        }
        Ok(())
    }
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        // The cache lock in reset waits for every opener which passed admission.
        // Once closed, an admitted Run holding LocalNode can never open afterward.
        self.closed.store(true, Ordering::SeqCst);
        self.reset().await?;
        let result = self.chain.shutdown().await;
        #[cfg(test)]
        self.drained.notify_one();
        result
    }
    #[cfg(test)]
    pub(crate) fn chain_initialized(&self) -> bool {
        self.chain.initialized()
    }
}
