//! Paid client state is owned by the requesting user. Provider payloads never
//! pass through this module or Gate's local request history.
use anyhow::{Context as _, bail};
use hellas_rpc::protocol::{
    work_fetch::PreparedPaidFetchInputV1, work_profile::PreparedPaidWorkInput,
};
use hellas_sdk::{
    client::{FetchExecutionEvent, FetchOutcome, ProviderTrustAnchor},
    kernel::{CoinId, EdgeId, Funding, List, MAX_PARTY_INPUTS, Secp256k1Signer},
};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    work_config: PathBuf,
    journal_root: PathBuf,
    bond: String,
    payment_coins: Vec<String>,
    omission_bond: u64,
    #[serde(default)]
    settle: bool,
}

#[allow(clippy::too_many_arguments)]
pub async fn run(
    path: &Path,
    request: &crate::dto::RunRequest,
    identity: &hellas_sdk::ClientIdentity,
    provider: hellas_sdk::iroh::EndpointId,
    provider_addrs: Vec<SocketAddr>,
    environment: hellas_rpc::ContentId,
    assurance: hellas_rpc::Assurance,
    trust: ProviderTrustAnchor,
) -> anyhow::Result<Vec<FetchExecutionEvent>> {
    let config: Config = serde_json::from_slice(&std::fs::read(path)?)
        .context("invalid paid client configuration")?;
    let work = hellas_sdk::work_config::load_work_config(&config.work_config)?;
    let manifest = [
        hellas_rpc::FetchEnvironment::Http,
        hellas_rpc::FetchEnvironment::OpenAiResponses,
        hellas_rpc::FetchEnvironment::CodexResponses,
    ]
    .into_iter()
    .find(|env| env.manifest_id() == environment)
    .context("unsupported paid Fetch environment")?
    .manifest();
    if environment == hellas_rpc::FetchEnvironment::Http.manifest_id() {
        hellas_rpc::http_fetch::HttpFetchRequest::decode(request.input.as_bytes())?;
    }
    anyhow::ensure!(
        !config.payment_coins.is_empty() && config.payment_coins.len() <= MAX_PARTY_INPUTS,
        "paid client requires one to four funding coins"
    );
    let coins = config
        .payment_coins
        .iter()
        .map(|s| parse_hex(s).map(CoinId::from_bytes))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let funding = Funding::new(
        List::new(
            std::array::from_fn(|i| coins.get(i).copied().unwrap_or(CoinId::from_bytes([0; 32]))),
            coins.len(),
        )
        .context("invalid funding coins")?,
        List::take([CoinId::from_bytes([0; 32]); MAX_PARTY_INPUTS], 0),
    );
    let bond = EdgeId::from_bytes(parse_hex(&config.bond)?);
    let signer = Secp256k1Signer::from_secret_scalar(identity.caller_secret_bytes())
        .map_err(|_| anyhow::anyhow!("invalid client settlement identity"))?;
    // Preserve the caller's nonce across a retry. The provider can therefore
    // recognize one job without re-running a side-effecting upstream request.
    std::fs::create_dir_all(&config.journal_root)?;
    let pending = config
        .journal_root
        .join(format!("pending-{}.fetch", hex32(&bond.to_bytes())));
    let prepared = if pending.exists() {
        anyhow::ensure!(
            std::fs::metadata(&pending)?.len() <= 4 * 1024 * 1024,
            "pending request exceeds its byte limit"
        );
        let bytes = std::fs::read(&pending)?;
        let bundle = PreparedPaidFetchInputV1::decode(&bytes, 4 * 1024 * 1024)?;
        let input =
            hellas_rpc::fetch::verify_input_events(&bundle.parts()?.fetch_input_transcript)?;
        anyhow::ensure!(
            input.caller_key == identity.caller_key().public_key()
                && input.execution_environment == environment
                && input.assurance == assurance
                && input.service == request.service
                && input.method == request.method
                && input.body.as_bytes() == request.input.as_bytes(),
            "a different paid request is pending in this client journal; resume it before starting another"
        );
        bundle
    } else {
        let input = hellas_rpc::fetch::build_input_events_with_retention(
            &request.service,
            &request.method,
            request.input.as_bytes(),
            environment,
            assurance,
            identity.caller_key(),
            hellas_rpc::Retention::Ephemeral,
        )?;
        let bundle = PreparedPaidFetchInputV1::new(&input, &manifest)?;
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new_in(&config.journal_root)?;
        file.write_all(&bundle.encode()?)?;
        file.as_file().sync_all()?;
        file.persist_noclobber(&pending)
            .map_err(|error| error.error)?;
        #[cfg(unix)]
        std::fs::File::open(&config.journal_root)?.sync_all()?;
        bundle
    };
    let result = hellas_sdk::paid_client::run_paid_work(
        hellas_sdk::paid_client::PaidWorkRun {
            config: work,
            journal_root: config.journal_root,
            provider,
            provider_addrs,
            provider_trust: Some(trust),
            bond,
            payment_funding: funding,
            omission_bond: config.omission_bond,
            prepared_input: PreparedPaidWorkInput::Fetch(prepared),
            acceptance_blocks: 16,
            terminal_blocks: 64,
            payment_blocks: 32,
            timeout: std::time::Duration::from_secs(300),
            settle: config.settle,
        },
        identity.transport_key(),
        signer,
    )
    .await?;
    let envelopes =
        hellas_rpc::protocol::work::decode_transcript(&result.transcript, 4 * 1024 * 1024)?;
    let (terminal, events) = envelopes
        .split_last()
        .context("missing paid Fetch terminal")?;
    let mut decoded = Vec::new();
    let mut position = 0;
    for envelope in events {
        let payload = envelope.payload();
        position += payload.len() as u64;
        decoded.push(FetchExecutionEvent::Chunk {
            position,
            output_event: envelope.clone(),
            event: hellas_rpc::fetch::decode_fetch_event_payload(payload)?,
        });
    }
    let terminal = hellas_rpc::fetch::decode_fetch_terminal_payload(terminal.payload())?;
    decoded.push(FetchExecutionEvent::Done(FetchOutcome::Completed {
        output_events: envelopes,
        terminal,
    }));
    std::fs::remove_file(pending)?;
    Ok(decoded)
}

fn parse_hex<const N: usize>(text: &str) -> anyhow::Result<[u8; N]> {
    if text.len() != N * 2 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("expected a {N}-byte hexadecimal identifier");
    }
    let mut out = [0; N];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .context("invalid hexadecimal identifier")?;
    }
    Ok(out)
}
fn hex32(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderOffer {
    work_config: PathBuf,
    client: String,
    stake_coins: Vec<String>,
    bond_timeout: u64,
    timeout_payout: u64,
    max_job_price: u64,
}

pub async fn provision(
    path: &Path,
    identity: &hellas_sdk::ClientIdentity,
    preview: bool,
) -> anyhow::Result<String> {
    let offer: ProviderOffer = serde_json::from_slice(&std::fs::read(path)?)
        .context("invalid provider offer configuration")?;
    let options = hellas_sdk::work_provision::ProvisionOptions {
        work_config: hellas_sdk::work_config::load_work_config(&offer.work_config)?,
        settlement_key: Secp256k1Signer::from_secret_scalar(identity.caller_secret_bytes())
            .map_err(|_| anyhow::anyhow!("invalid provider settlement identity"))?,
        client: hellas_sdk::kernel::Key::from_bytes(parse_hex(&offer.client)?),
        stake_coins: offer
            .stake_coins
            .iter()
            .map(|value| parse_hex(value).map(CoinId::from_bytes))
            .collect::<anyhow::Result<_>>()?,
        bond_timeout: offer.bond_timeout,
        timeout_payout: offer.timeout_payout,
        max_job_price: offer.max_job_price,
    };
    if preview {
        return Ok(format!(
            "Bond: {}",
            hex32(&hellas_sdk::work_provision::preview_bond(&options)?.to_bytes())
        ));
    }
    let offer = hellas_sdk::work_provision::provision_offer(options).await?;
    Ok(format!(
        "Offer journaled. Bond: {}",
        hex32(&offer.bond_edge.to_bytes())
    ))
}
