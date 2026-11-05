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
use hellas_sdk::{
    kernel::{EdgeValues, NetworkId},
    work_config::{ChainCrossCheck, WorkConfig},
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub fn contract(presentation: FetchPresentation) -> anyhow::Result<hellas_rpc::FetchContract> {
    Ok(match presentation {
        FetchPresentation::Responses => hellas_providers::responses_contract()?,
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
        min_omit_response_blocks: input.min_omit_response_blocks,
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

pub fn config(
    chain: &ChainInput,
    poll_ms: u64,
    max_observation_age_ms: u64,
    journal_root: PathBuf,
) -> anyhow::Result<WorkConfig> {
    let genesis = if chain.committee.is_empty() {
        None
    } else {
        Some(hellas_genesis::Genesis {
            schema_version: hellas_genesis::GENESIS_SCHEMA_VERSION,
            network_id: chain.network.clone(),
            validators: chain
                .committee
                .iter()
                .map(|v| hellas_genesis::GenesisValidator {
                    public_key: v.public_key.clone(),
                    label: v.label.clone(),
                })
                .collect(),
            allocations: chain
                .allocations
                .iter()
                .map(|v| hellas_genesis::GenesisAllocation {
                    address: v.address.clone(),
                    balance: v.balance,
                })
                .collect(),
        })
    };
    if let Some(genesis) = &genesis {
        genesis.validate()?;
    }
    let config = WorkConfig {
        chain: ChainCrossCheck {
            network: NetworkId::new(&chain.network).context("invalid network identifier")?,
            genesis_payload_digest: chain.genesis_digest.parse()?,
            threshold_identity: hex::decode(&chain.threshold_identity)?,
            genesis,
        },
        validators: chain.validators.clone(),
        journal_root,
        poll: Duration::from_millis(poll_ms),
        max_observation_age: Duration::from_millis(max_observation_age_ms),
    };
    Ok(config.validate()?)
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
pub fn pool_options(
    directory: &Path,
    pool: &PaidPoolInput,
) -> anyhow::Result<hellas_sdk::paid_gateway::PaidGatewayOptions> {
    Ok(pool_snapshot(directory, pool)?.options)
}
pub(crate) struct PoolSnapshot {
    pub options: hellas_sdk::paid_gateway::PaidGatewayOptions,
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
    let pool_id = &pool.id;
    let mut providers = Vec::new();
    let mut snapshots = std::collections::BTreeMap::<String, trust::Snapshot>::new();
    let mut approvals = Vec::new();
    for input in &pool.providers {
        let provider: hellas_sdk::client::ProviderAddress = input.provider.parse()?;
        FetchRoutePolicy::sealed_route(&input.policy.service, &input.policy.method)?;
        let config = config(
            &input.policy.chain,
            input.policy.poll_ms,
            input.policy.max_observation_age_ms,
            directory
                .join("paid-channels")
                .join(pool_id)
                .join(provider.node.to_string()),
        )?;
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
        approvals.push(approved.record);
        let args = hellas_sdk::paid_client::PaidWorkOptions {
            config: config.clone(),
            journal_root: config.journal_root.clone(),
            provider,
            provider_addrs: input
                .addresses
                .iter()
                .map(|a| a.parse())
                .collect::<Result<_, _>>()?,
            assurance: approved.anchor.required_assurance,
            apple_trust: (approved.anchor.required_assurance
                == hellas_rpc::Assurance::AppleAppAttest)
                .then_some(approved.anchor),
            expected_price: input.policy.fixed_price,
            acceptance_blocks: input.acceptance_blocks,
            terminal_blocks: input.terminal_blocks,
            payment_blocks: input.payment_blocks,
            timeout: Some(Duration::from_secs(input.timeout_seconds)),
        };
        args.validate()?;
        providers.push(args);
    }
    let options = hellas_sdk::paid_gateway::PaidGatewayOptions {
        providers,
        max_pending_requests: pool.max_pending_requests,
    };
    options.validate()?;
    Ok(PoolSnapshot { options, approvals })
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
    let channels = directory.join("paid-channels");
    if channels.try_exists()? {
        for entry in std::fs::read_dir(channels)? {
            let path = entry?.path();
            if path
                .file_name()
                .is_some_and(|name| name == pool.id.as_str())
            {
                continue;
            }
            for provider in &providers {
                ensure!(
                    !path.join(provider.to_string()).try_exists()?,
                    "provider has retained channel state in another paid pool; reuse that pool"
                );
            }
        }
    }
    Ok(())
}
pub fn validate_pool(directory: &Path, input: &PaidPoolInput) -> anyhow::Result<()> {
    pool_options(directory, input)?;
    Ok(())
}
pub fn save_pool(directory: &Path, input: PaidPoolInput) -> anyhow::Result<()> {
    validate_pool(directory, &input)?;
    vault::write(&pool_path(directory, &input.id)?, &input)
}

pub fn serve_config(directory: &Path, input: &PaidServeInput) -> anyhow::Result<WorkConfig> {
    standing_policy(&input.policy)?;
    config(
        &input.policy.chain,
        input.policy.poll_ms,
        input.policy.max_observation_age_ms,
        directory.join("paid-provider"),
    )
}

pub fn canonical_serve(
    directory: &Path,
    mut input: PaidServeInput,
) -> anyhow::Result<PaidServeInput> {
    input.policy.chain.validators = serve_config(directory, &input)?.validators;
    Ok(input)
}
