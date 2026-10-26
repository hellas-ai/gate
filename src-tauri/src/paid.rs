//! Typed paid settings. SDK validation, signing, funding and settlement remain shared.
use crate::{dto::*, trust, vault};
use anyhow::{Context, ensure};
use hellas_sdk::work::model::{
    fetch::{FetchPolicyV2, FetchRoutePolicy, fetch_route_commitment},
    profile::WorkPolicy,
    work::{JobPaymentPolicyV2, PaidChannelPolicyV1},
};
use hellas_sdk::{
    kernel::{EdgeValues, Fees, NetworkId},
    work_config::{ChainCrossCheck, WorkConfig, WorkRoutes},
};
use std::{path::Path, time::Duration};

pub fn work_policy(
    presentation: FetchPresentation,
    service: &str,
    method: &str,
) -> anyhow::Result<WorkPolicy> {
    let contract = match presentation {
        FetchPresentation::Responses => hellas_reseller::responses_contract()?,
        FetchPresentation::Http => hellas_rpc::FetchContract::http()?,
    };
    let route = FetchRoutePolicy::sealed_route(service, method)?;
    let policy = WorkPolicy::Fetch {
        policy: FetchPolicyV2 {
            allowed_environment: contract.manifest_id(),
            route_commitment: fetch_route_commitment(&route.canonical_body_bytes())?,
            max_request_body_bytes: 64 << 10,
            max_output_events: 4096,
            max_output_bytes: 4 << 20,
            max_spool_bytes: 8 << 20,
            max_encoded_result_frame: 4 << 20,
            max_encoded_prepared_input: 128 << 10,
        },
        route,
        contract,
    };
    policy.check()?;
    Ok(policy)
}
pub fn config(
    input: &PaidPolicyInput,
    journal_root: std::path::PathBuf,
) -> anyhow::Result<WorkConfig> {
    ensure!(
        input.poll_ms > 0 && input.max_observation_age_ms > 0,
        "paid polling and progress limits must be positive"
    );
    ensure!(
        input.chain.validators.len() == hellas_sdk::work_config::VALIDATOR_COUNT,
        "configure all six validator RPC addresses"
    );
    let genesis = if input.chain.committee.is_empty() {
        None
    } else {
        Some(hellas_genesis::Genesis {
            schema_version: hellas_genesis::GENESIS_SCHEMA_VERSION,
            network_id: input.chain.network.clone(),
            validators: input
                .chain
                .committee
                .iter()
                .map(|v| hellas_genesis::GenesisValidator {
                    public_key: v.public_key.clone(),
                    label: v.label.clone(),
                })
                .collect(),
            allocations: input
                .chain
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
            network: NetworkId::new(&input.chain.network).context("invalid network identifier")?,
            genesis_payload_digest: input.chain.genesis_digest.parse()?,
            threshold_identity: hex::decode(&input.chain.threshold_identity)?,
            genesis,
        },
        validators: input.chain.validators.clone(),
        journal_root,
        routes: WorkRoutes::default(),
        policy_salt: trust::fixed(&input.policy_salt)?,
        channel_policy: PaidChannelPolicyV1 {
            compute_credit_limit: input.compute_credit_limit,
            delivery_credit_limit: input.delivery_credit_limit,
        },
        work_policy: work_policy(input.presentation, &input.service, &input.method)?,
        payment_policy: JobPaymentPolicyV2 {
            fixed_price: input.fixed_price,
            dispatch_margin_blocks: input.dispatch_blocks,
            delivery_margin_blocks: input.delivery_blocks,
            oracle_grace_blocks: input.oracle_blocks,
        },
        poll: Duration::from_millis(input.poll_ms),
        max_observation_age: Duration::from_millis(input.max_observation_age_ms),
        expected_payment_values: EdgeValues::new(
            input.value,
            input.reserve,
            Fees::new(
                input.fee_base,
                input.fee_slot,
                input.fee_proof,
                input.fee_lifetime,
            ),
        ),
        min_omit_response_blocks: input.min_omit_response_blocks,
    };
    config.node_config(config.journal_root.join("validation"), None)?;
    config.work_policy.check()?;
    config.payment_policy.check()?;
    Ok(config)
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
    ensure!(
        !pool.providers.is_empty() && pool.providers.len() <= 16,
        "select 1 to 16 paid providers"
    );
    let pool_id = &pool.id;
    let mut providers = Vec::new();
    for input in &pool.providers {
        let config = config(
            &input.policy,
            directory
                .join("paid-channels")
                .join(pool_id)
                .join(&input.bond),
        )?;
        let approved = trust::resolve(
            directory,
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
                hellas_sdk::kernel::List::empty(hellas_sdk::kernel::CoinId::from_bytes([0; 32])),
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
    let options = hellas_sdk::paid_gateway::PaidGatewayOptions {
        providers,
        max_pending_requests: pool.max_pending_requests,
    };
    options.validate()?;
    Ok(options)
}
pub fn save_pool(directory: &Path, input: PaidPoolInput) -> anyhow::Result<()> {
    pool_options(directory, &input)?;
    vault::write(&pool_path(directory, &input.id)?, &input)
}

/// The checked SDK constructor is the only route representation Gate builds.
pub fn serve_config(directory: &Path, input: &PaidServeInput) -> anyhow::Result<WorkConfig> {
    ensure!(
        input.routes.len() <= 16,
        "at most 16 paid routes are supported"
    );
    let mut config = config(&input.policy, directory.join("paid-provider"))?;
    config.routes = WorkRoutes::try_from_iter(
        input
            .routes
            .iter()
            .map(|r| {
                Ok(hellas_sdk::work_config::WorkRoute {
                    peer: hellas_wire::peers::PeerId::from_bytes(
                        *r.peer.parse::<hellas_sdk::iroh::EndpointId>()?.as_bytes(),
                    ),
                    bond: hellas_sdk::kernel::EdgeId::from_bytes(trust::fixed(&r.bond)?),
                    client: hellas_sdk::kernel::Key::from_bytes(trust::fixed(&r.client)?),
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?,
    )?;
    Ok(config)
}
pub fn canonical_serve(directory: &Path, input: PaidServeInput) -> anyhow::Result<PaidServeInput> {
    let config = serve_config(directory, &input)?;
    Ok(PaidServeInput {
        policy: input.policy,
        routes: config
            .routes
            .iter()
            .map(|route| PaidRouteInput {
                peer: hex::encode(route.peer.as_bytes()),
                bond: hex::encode(route.bond.to_bytes()),
                client: hex::encode(route.client.to_bytes()),
            })
            .collect(),
    })
}
async fn provision_options(
    local: &crate::control::LocalNode,
    input: &ProvisionInput,
) -> anyhow::Result<hellas_sdk::work_provision::ProvisionOptions> {
    let view = local.composition.view()?;
    ensure!(
        view.revision == input.expected_provider_revision,
        "provider revision conflict; preview again"
    );
    let paid = view
        .paid
        .context("configure typed paid Serve policy first")?;
    let (client, client_node) =
        crate::contacts::confirmed_device(&local.directory, &input.contact, &input.client_node)?;
    ensure!(
        input.addresses.len() <= 16
            && input
                .addresses
                .iter()
                .all(|a| a.len() <= 512 && a.parse::<std::net::SocketAddr>().is_ok()),
        "invalid provider socket addresses"
    );
    let provider = local
        .coordinator
        .provider()
        .await?
        .context("bind a provider first")?;
    Ok(hellas_sdk::work_provision::ProvisionOptions {
        work_config: serve_config(&local.directory, &paid)?,
        settlement_key: hellas_sdk::kernel::Secp256k1Signer::from_secret_scalar(
            local.identity.node.node_secret_bytes(),
        )
        .map_err(|_| anyhow::anyhow!("invalid provider settlement key"))?,
        provider,
        addresses: input.addresses.clone(),
        client,
        client_node,
        redelegate_payout: input.redelegate_payout,
        stake_coins: input
            .stake_coins
            .iter()
            .map(|c| Ok(hellas_sdk::kernel::CoinId::from_bytes(trust::fixed(c)?)))
            .collect::<anyhow::Result<_>>()?,
        bond_timeout: input.bond_timeout,
        timeout_payout: input.timeout_payout,
        max_job_price: input.max_job_price,
    })
}
pub async fn provision(
    local: &crate::control::LocalNode,
    input: ProvisionInput,
    commit: bool,
) -> anyhow::Result<ProvisionView> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let session = local.session(false).await?;
    let status = session.status().await?;
    let mut options = provision_options(local, &input).await?;
    let bond = hellas_sdk::work_provision::preview_bond(&options, &local.coordinator).await?;
    let peer: hellas_sdk::iroh::EndpointId = input.client_peer.parse()?;
    let route = hellas_sdk::work_config::WorkRoute {
        peer: hellas_wire::peers::PeerId::from_bytes(*peer.as_bytes()),
        bond,
        client: options.client_node,
    };
    // Preserve existing routes and refuse conflicting bond bindings through the SDK.
    if let Some(held) = options.work_config.routes.iter().find(|r| r.bond == bond) {
        ensure!(
            held == &route,
            "candidate bond has a different configured route"
        );
    } else {
        options.work_config.routes =
            WorkRoutes::try_from_iter(options.work_config.routes.iter().cloned().chain([route]))?;
    }
    let mut view = ProvisionView {
        bond: hex::encode(bond.to_bytes()),
        owner: local.coordinator.owner().await?.to_string(),
        provider_node: hex::encode(options.settlement_key.party_key().to_bytes()),
        client: options.client.principal().to_string(),
        client_node: hex::encode(options.client_node.to_bytes()),
        payout_delegate: input
            .redelegate_payout
            .then(|| hex::encode(options.settlement_key.party_key().to_bytes())),
        timeout_payout: input.timeout_payout,
        bond_timeout: input.bond_timeout,
        stake_coins: input.stake_coins,
        signed_offer: None,
    };
    if commit {
        ensure!(
            status.provider.is_none_or(|p| p.state != 2),
            "stop and drain the provider before provisioning; existing setup journals have one exclusive owner"
        );
        let node = local.chain.get(&options.work_config).await?;
        let provisioned =
            hellas_sdk::work_provision::provision_offer(options, &node, &local.coordinator).await?;
        view.signed_offer = Some(STANDARD.encode(provisioned.offer.encode()?));
    }
    Ok(view)
}
