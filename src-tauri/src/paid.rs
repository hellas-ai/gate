//! Standing paid terms and requester settings use the SDK's authority and funding owner.
use crate::{dto::*, trust, vault};
use anyhow::{Context, ensure};
use hellas_channel::model::{
    paid_policy::{OfferPolicy, PaidPolicy},
    work::{JobPaymentPolicyV2, PaidChannelPolicyV1},
};
use hellas_rpc::protocol::invocation::{
    Admission, InvocationPolicy, Limits,
    fetch::{FetchAdmission, FetchLimits, FetchRoutePolicy},
};
use hellas_sdk::{kernel::EdgeValues, work_config::WorkConfig};
use std::{path::Path, time::Duration};

pub fn contract(presentation: FetchPresentation) -> anyhow::Result<hellas_rpc::FetchContract> {
    Ok(match presentation {
        FetchPresentation::Responses => {
            hellas_adaptors::openai::responses::fetch::responses_contract()?
        }
        FetchPresentation::Http => hellas_rpc::FetchContract::http()?,
    })
}

pub fn standing_policy(input: &PaidPolicyInput) -> anyhow::Result<PaidPolicy> {
    ensure!(
        input.bond_lifetime_blocks <= hellas_chain::domain::MAX_EDGE_LIFETIME_BLOCKS,
        "bond lifetime exceeds the chain maximum"
    );
    let capacity = input
        .fixed_price
        .checked_mul(input.jobs_per_channel)
        .context("paid channel capacity overflow")?;
    let value = capacity
        .checked_add(input.omission_bond)
        .context("paid funding overflow")?;
    let limits = &input.limits;
    let policy = PaidPolicy {
        channel_policy: PaidChannelPolicyV1 {
            compute_credit_limit: input.compute_credit_limit,
            delivery_credit_limit: input.delivery_credit_limit,
        },
        invocation_policy: InvocationPolicy {
            admissions: vec![Admission::Fetch {
                admission: FetchAdmission {
                    contract: contract(input.presentation)?,
                    route: FetchRoutePolicy::sealed_route(&input.service, &input.method)?,
                    limits: FetchLimits {
                        max_request_body_bytes: limits.max_request_body_bytes,
                        max_output_events: limits.max_output_events,
                        max_spool_bytes: limits.max_spool_bytes,
                        max_encoded_result_frame: limits.max_encoded_result_frame,
                        max_encoded_prepared_input: limits.max_encoded_prepared_input,
                    },
                },
            }],
            limits: Limits {
                max_upload_bytes: u64::from(limits.max_encoded_prepared_input),
                max_output_bytes: limits.max_output_bytes,
                max_steps: 1,
                execution_timeout_ms: limits.execution_timeout_ms,
            },
        },
        payment_policy: JobPaymentPolicyV2 {
            fixed_price: input.fixed_price,
            dispatch_margin_blocks: input.dispatch_blocks,
            delivery_margin_blocks: input.delivery_blocks,
            oracle_grace_blocks: input.oracle_blocks,
        },
        expected_payment_values: EdgeValues::new(value, 0, hellas_chain::domain::KERNEL_FEES),
        jobs_per_channel: input.jobs_per_channel,
        offers: OfferPolicy {
            max_stake_per_client: input.max_stake_per_client,
            bond_lifetime_blocks: input.bond_lifetime_blocks,
            max_open_offers: input.max_open_offers,
            omission_bond: input.omission_bond,
            redelegate_payout: input.redelegate_payout,
        },
    };
    policy.check()?;
    Ok(policy)
}

pub fn home(directory: &Path) -> hellas_private::StoreLocation {
    hellas_private::StoreLocation::from_home(directory, hellas_private::Durability::Power)
}

pub fn profile(input: &ChainInput) -> anyhow::Result<hellas_genesis::ChainProfile> {
    Ok(match input {
        ChainInput::Devnet => hellas_genesis::ChainProfile::known(hellas_genesis::Chain::Devnet),
        ChainInput::Custom { file } => {
            hellas_sdk::work_config::load_chain_profile(Path::new(file))?
        }
    })
}

pub fn config(chain: &ChainInput, directory: &Path) -> anyhow::Result<WorkConfig> {
    Ok(WorkConfig::for_chain(profile(chain)?, &home(directory))?)
}

pub fn pool_path(directory: &Path, id: &str) -> anyhow::Result<std::path::PathBuf> {
    ensure!(
        !id.is_empty()
            && id.len() <= 64
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "invalid paid pool name"
    );
    Ok(directory.join("paid-pools").join(format!("{id}.json")))
}
pub(crate) struct ProviderSnapshot {
    pub input: PaidProviderInput,
    pub provider: hellas_sdk::client::ProviderAddress,
    pub config: WorkConfig,
    pub trust: trust::Snapshot,
}
pub(crate) struct PoolSnapshot {
    pub providers: Vec<ProviderSnapshot>,
    pub approvals: Vec<hellas_sdk::trust_policy::ApprovalRecord>,
}
pub(crate) fn pool_snapshot(
    directory: &Path,
    pool: &PaidPoolInput,
) -> anyhow::Result<PoolSnapshot> {
    pool_path(directory, &pool.id)?;
    validate_assignments(directory, pool)?;
    ensure!(
        !pool.providers.is_empty() && pool.providers.len() <= 16,
        "select 1 to 16 paid providers"
    );
    let mut providers = Vec::new();
    let mut snapshots = std::collections::BTreeMap::<String, trust::Snapshot>::new();
    let mut approvals = Vec::new();
    let mut nodes = std::collections::BTreeSet::new();
    for input in &pool.providers {
        let provider: hellas_sdk::client::ProviderAddress = input.provider.parse()?;
        ensure!(nodes.insert(provider.node), "duplicate paid provider");
        FetchRoutePolicy::sealed_route(&input.policy.service, &input.policy.method)?;
        let config = config(&input.policy.chain, directory)?;
        let approved = match snapshots.entry(input.trust_policy.clone()) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.get().clone(),
            std::collections::btree_map::Entry::Vacant(entry) => entry
                .insert(trust::snapshot(directory, &input.trust_policy)?)
                .clone(),
        };
        ensure!(
            approved.anchor.expected_genesis == provider.enrollment,
            "paid provider address differs from the approved enrollment"
        );
        ensure!(
            input.policy.fixed_price > 0
                && input.acceptance_blocks > 0
                && input.terminal_blocks > 0
                && input.payment_blocks > 0
                && input.timeout_seconds > 0,
            "price and deadline spans must be positive"
        );
        for address in &input.addresses {
            address.parse::<std::net::SocketAddr>()?;
        }
        approvals.push(approved.record.clone());
        providers.push(ProviderSnapshot {
            input: input.clone(),
            provider,
            config,
            trust: approved,
        });
    }
    Ok(PoolSnapshot {
        providers,
        approvals,
    })
}

impl ProviderSnapshot {
    pub(crate) fn journal_root(&self) -> hellas_private::StoreLocation {
        self.config.chain.gateway(&self.provider.node.to_string())
    }
    pub async fn options(&self) -> anyhow::Result<hellas_sdk::paid_client::PaidWorkOptions> {
        use hellas_store::content::{DiskStore, IndexMode, Reservations};
        let input = &self.input;
        let provider: hellas_sdk::client::ProviderAddress = input.provider.parse()?;
        let root = self.journal_root();
        let content = DiskStore::open(
            root.child("content"),
            vec![],
            IndexMode::Resume,
            None,
            Reservations::ram(hellas_channel::content::DEFAULT_MAX_STAGING_BYTES),
            false,
        )
        .await?;
        let recovered: anyhow::Result<()> = async {
            hellas_sdk::paid_client::recover_content(&root, self.config.network(), &content)
                .await?;
            content.finish_recovery().await?;
            Ok(())
        }
        .await;
        if let Err(error) = recovered {
            content.close().await;
            return Err(error);
        }
        let args = hellas_sdk::paid_client::PaidWorkOptions {
            config: self.config.clone(),
            journal_kind: hellas_sdk::paid_client::PaidJournalKind::Gateway,
            content,
            provider,
            provider_addrs: input
                .addresses
                .iter()
                .map(|a| a.parse())
                .collect::<Result<_, _>>()?,
            assurance: self.trust.anchor.required_assurance,
            apple_trust: (self.trust.anchor.required_assurance
                == hellas_rpc::Assurance::AppleAppAttest)
                .then(|| self.trust.anchor.clone()),
            expected_price: input.policy.fixed_price,
            acceptance_blocks: input.acceptance_blocks,
            terminal_blocks: input.terminal_blocks,
            payment_blocks: input.payment_blocks,
            timeout: Some(Duration::from_secs(input.timeout_seconds)),
        };
        if let Err(error) = args.validate() {
            args.content.close().await;
            return Err(error.into());
        }
        Ok(args)
    }
}
fn validate_assignments(directory: &Path, pool: &PaidPoolInput) -> anyhow::Result<()> {
    let providers = pool
        .providers
        .iter()
        .map(|p| {
            p.provider
                .parse::<hellas_sdk::client::ProviderAddress>()
                .map(|address| address.node)
        })
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    let pools = directory.join("paid-pools");
    if pools.try_exists()? {
        for entry in std::fs::read_dir(pools)? {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "json")
                || path == pool_path(directory, &pool.id)?
            {
                continue;
            }
            let other: PaidPoolInput = vault::read(&path)?.context("paid pool disappeared")?;
            for provider in other.providers {
                ensure!(
                    !providers.contains(
                        &provider
                            .provider
                            .parse::<hellas_sdk::client::ProviderAddress>()?
                            .node
                    ),
                    "provider is already assigned to paid pool {}; use that pool",
                    other.id
                );
            }
        }
    }
    Ok(())
}
pub fn validate_pool(directory: &Path, input: &PaidPoolInput) -> anyhow::Result<()> {
    pool_snapshot(directory, input)?;
    Ok(())
}
pub fn save_pool(directory: &Path, input: PaidPoolInput) -> anyhow::Result<()> {
    validate_pool(directory, &input)?;
    vault::write(&pool_path(directory, &input.id)?, &input)
}

pub fn serve_config(directory: &Path, input: &PaidServeInput) -> anyhow::Result<WorkConfig> {
    standing_policy(&input.policy)?;
    config(&input.policy.chain, directory)
}

/// Remember every provider chain before it can create a journal in the shared store.
pub(crate) fn retain_provider_profile(config: &WorkConfig) -> anyhow::Result<()> {
    let chain = &config.chain;
    vault::write(
        &chain.provider().join("profile.json"),
        &serde_json::json!({
            "trust": chain.trust(), "validators": chain.validators(), "genesis_json": chain.genesis_json(),
        }),
    )
}

/// Recover inactive chains too: disabling paid Serve must not discard retained work.
pub(crate) async fn recover_provider_content(
    directory: &Path,
    identity: &crate::identity::Identity,
    content: &hellas_store::content::DiskStore,
) -> anyhow::Result<()> {
    use hellas_sdk::paid_provider::{MountedSetup, MountedWork, WorkRunner, WorkRunnerConfig};
    let chains = directory.join("chains");
    if chains.try_exists()? {
        for entry in std::fs::read_dir(chains)? {
            let entry = entry?;
            let path = entry.path().join("provider/profile.json");
            if !path.try_exists()? {
                ensure!(
                    !entry.path().join("provider/channels").try_exists()?,
                    "retained provider journals require their chain profile"
                );
                continue;
            }
            let profile = hellas_sdk::work_config::load_chain_profile(&path)?;
            let config = WorkConfig::for_chain(profile, &home(directory))?;
            ensure!(
                config.chain.as_path() == entry.path(),
                "provider profile does not match its chain directory"
            );
            let settlement_key = hellas_sdk::kernel::Secp256k1Signer::from_secret_scalar(
                identity.node.node_secret_bytes(),
            )
            .map_err(|_| anyhow::anyhow!("invalid provider settlement key"))?;
            // Canonical replay restores pins and reservations in the shared DiskStore.
            // Dropping the offline runner releases journal writers before Serve opens.
            let recovered = WorkRunner::discover(
                WorkRunnerConfig {
                    chain: config.chain,
                    settlement_key,
                },
                MountedWork::default(),
                MountedSetup::with_content_store(content.clone()),
            )
            .await?;
            drop(recovered);
        }
    }
    content.finish_recovery().await?;
    Ok(())
}
