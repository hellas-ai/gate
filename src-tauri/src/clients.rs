//! Run and gateway clients consume the same approved SDK backends and funding.
use crate::{
    contacts, control::LocalNode, credentials::PreparedIssue, dto::*, identity, paid, trust, vault,
};
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
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

pub struct Target {
    pub backend: Arc<dyn WorkExecutionBackend>,
    pub provider: hellas_sdk::iroh::EndpointId,
    pub service: String,
    pub method: String,
    pub presentation: FetchPresentation,
}
pub struct Clients {
    directory: PathBuf,
    chain: crate::chain::ChainNode,
    cache: Mutex<BTreeMap<String, Arc<Target>>>,
    paid: Mutex<BTreeMap<String, Arc<dyn WorkExecutionBackend>>>,
}
impl Clients {
    pub fn new(directory: &Path) -> Self {
        Self {
            directory: directory.to_owned(),
            chain: crate::chain::ChainNode::new(directory.join("chain")),
            cache: Mutex::new(BTreeMap::new()),
            paid: Mutex::new(BTreeMap::new()),
        }
    }
    pub async fn target(
        &self,
        local: &LocalNode,
        input: &WorkClientConfig,
    ) -> anyhow::Result<Arc<Target>> {
        let policy = trust::store(&self.directory).load(&input.trust_policy)?;
        let key = format!("{}:{}", serde_json::to_string(input)?, policy.revision);
        let mut cache = self.cache.lock().await;
        if let Some(target) = cache.get(&key) {
            return Ok(target.clone());
        }
        let target = match &input.target {
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
                    .context("selected resource is absent from this allowance")?;
                let hellas_sdk::work::model::profile::WorkPolicy::Fetch {
                    route:
                        hellas_sdk::work::model::fetch::FetchRoutePolicy::SealedRoute {
                            service,
                            method,
                        },
                    contract,
                    ..
                } = &selected.work
                else {
                    anyhow::bail!("select a named Fetch resource");
                };
                let (service, method) = (service.clone(), method.clone());
                let presentation =
                    if contract.schema() == hellas_rpc::FetchSchema::StatelessResponsesV1 {
                        FetchPresentation::Responses
                    } else {
                        FetchPresentation::Http
                    };
                let approved = trust::resolve(
                    &self.directory,
                    &input.trust_policy,
                    selected.work.allowed_environment(),
                )?;
                let provider = hellas_sdk::iroh::EndpointId::from_bytes(
                    &hellas_sdk::work::model::grant::records::grant_transport(
                        &offer.offer().provider,
                    )?,
                )?;
                let pinned =
                    UnpinnedOffer::decode(&offer.encode()?, UnixMillis::now()?)?.pin(&approved)?;
                let credential = identity::decode_credential(
                    &local
                        .vault
                        .required(&PreparedIssue::key(access.credential_id))
                        .await?,
                )?;
                let application = Arc::new(hellas_sdk::node_control::profile::ClientProfile {
                    authority: access.authority,
                    authorization: access.authorization,
                    credential,
                    local: None,
                });
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
                            .join(access_id)
                            .join(allowance),
                        timeout: Duration::from_secs(300),
                    },
                    transport,
                    Some(resource.clone()),
                )
                .await?;
                Target {
                    backend,
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
                let pool =
                    vault::read::<PaidPoolInput>(&paid::pool_path(&self.directory, pool_id)?)?
                        .context("configure a typed paid pool first")?;
                let mut providers = Vec::new();
                for input in &pool.providers {
                    let config = paid::config(
                        &input.policy,
                        self.directory
                            .join("paid-channels")
                            .join(pool_id)
                            .join(&input.bond),
                    )?;
                    let approved = trust::resolve(
                        &self.directory,
                        &input.trust_policy,
                        config.work_policy.allowed_environment(),
                    )?;
                    let coins = input
                        .coins
                        .iter()
                        .map(|s| Ok(hellas_sdk::kernel::CoinId::from_bytes(trust::fixed(s)?)))
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    let args = hellas_sdk::paid_client::PaidWorkOptions {
                        config: config.clone(),
                        journal_root: config.journal_root.clone(),
                        provider: input.provider.parse()?,
                        provider_addrs: input
                            .addresses
                            .iter()
                            .map(|a| a.parse())
                            .collect::<Result<_, _>>()?,
                        provider_trust: approved,
                        bond: hellas_sdk::kernel::EdgeId::from_bytes(trust::fixed(&input.bond)?),
                        payment_funding: hellas_sdk::kernel::Funding::new(
                            hellas_sdk::work_config::coin_list(&coins)
                                .context("select 1 to 4 payment coins")?,
                            hellas_sdk::kernel::List::empty(
                                hellas_sdk::kernel::CoinId::from_bytes([0; 32]),
                            ),
                        ),
                        omission_bond: input.omission_bond,
                        acceptance_blocks: input.acceptance_blocks,
                        terminal_blocks: input.terminal_blocks,
                        payment_blocks: input.payment_blocks,
                        timeout: Duration::from_secs(input.timeout_seconds),
                    };
                    args.validate()?;
                    providers.push(args);
                }
                let selected = pool
                    .providers
                    .iter()
                    .find(|p| p.provider == *provider)
                    .context("provider is absent from paid pool")?;
                ensure!(
                    selected.trust_policy == input.trust_policy,
                    "Run trust policy must match selected pool provider"
                );
                ensure!(
                    *service == selected.policy.service && *method == selected.policy.method,
                    "paid route differs from selected policy"
                );
                let first = providers.first().context("paid pool is empty")?;
                let chain = self.chain.get(&first.config).await?;
                let pool_key = serde_json::to_string(&pool)?;
                let mut paid_cache = self.paid.lock().await;
                let backend = if let Some(backend) = paid_cache.get(&pool_key) {
                    backend.clone()
                } else {
                    let transport = self.transport(local, &format!("paid:{pool_key}")).await?;
                    let identity = hellas_sdk::NodeIdentity::from_secret_bytes(
                        transport.to_bytes(),
                        local.identity.node.node_secret_bytes(),
                    )?;
                    let backend = hellas_sdk::paid_gateway::PaidGateway::open(
                        hellas_sdk::paid_gateway::PaidGatewayOptions {
                            providers,
                            max_pending_requests: pool.max_pending_requests,
                        },
                        identity,
                        chain,
                        local.coordinator.clone(),
                    )
                    .await?;
                    let backend: Arc<dyn WorkExecutionBackend> = backend;
                    paid_cache.insert(pool_key, backend.clone());
                    backend
                };
                Target {
                    backend,
                    provider: provider.parse()?,
                    service: service.clone(),
                    method: method.clone(),
                    presentation: selected.policy.presentation,
                }
            }
        };
        let target = Arc::new(target);
        cache.insert(key, target.clone());
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
    pub async fn reset(&self) -> anyhow::Result<()> {
        let held = std::mem::take(&mut *self.cache.lock().await);
        let mut failure = None;
        for target in held.values() {
            if let Err(e) = target.backend.drain().await {
                failure.get_or_insert(anyhow::Error::from(e));
            }
        }
        self.paid.lock().await.clear();
        if let Some(e) = failure {
            return Err(e);
        }
        Ok(())
    }
    pub async fn shutdown(&self) -> anyhow::Result<()> {
        self.reset().await?;
        self.chain.shutdown().await
    }
}
