//! Gate's provider provisioning boundary. Sessions and funding remain in the SDK.
use anyhow::Context as _;
use hellas_sdk::kernel::{CoinId, Key, Secp256k1Signer};
use std::path::{Path, PathBuf};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderOffer {
    work_config: PathBuf,
    client: String,
    stake_coins: Vec<String>,
    bond_timeout: u64,
    timeout_payout: u64,
    max_job_price: u64,
    #[serde(default)]
    addresses: Vec<std::net::SocketAddr>,
}

pub async fn provision(
    path: &Path,
    identity: &hellas_sdk::ClientIdentity,
    provider: hellas_rpc::ProviderEnrollmentBundle,
    preview: bool,
    chain: &crate::chain::ChainNode,
) -> anyhow::Result<String> {
    let file = std::fs::File::open(path)?;
    use std::io::Read;
    let mut bytes = Vec::new();
    file.take((1 << 20) + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= 1 << 20,
        "offer configuration exceeds its byte limit"
    );
    let config: ProviderOffer =
        serde_json::from_slice(&bytes).context("invalid paid offer configuration")?;
    let options = hellas_sdk::work_provision::ProvisionOptions {
        work_config: hellas_sdk::work_config::load_work_config(&config.work_config)?,
        settlement_key: Secp256k1Signer::from_secret_scalar(identity.caller_secret_bytes())
            .map_err(|_| anyhow::anyhow!("invalid settlement key"))?,
        provider,
        addresses: config.addresses.iter().map(ToString::to_string).collect(),
        client: Key::from_bytes(hex(&config.client)?),
        stake_coins: config
            .stake_coins
            .iter()
            .map(|s| hex(s).map(CoinId::from_bytes))
            .collect::<anyhow::Result<_>>()?,
        bond_timeout: config.bond_timeout,
        timeout_payout: config.timeout_payout,
        max_job_price: config.max_job_price,
    };
    if preview {
        return Ok(encode_hex(
            &hellas_sdk::work_provision::preview_bond(&options)?.to_bytes(),
        ));
    }
    let node = chain.get(&options.work_config).await?;
    let made = hellas_sdk::work_provision::provision_offer(options, &node).await?;
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "provider": identity.node_id().to_string(),
        "bond": encode_hex(&made.bond_edge.to_bytes()),
        "provider_genesis": made.offer,
    }))?)
}

fn hex<const N: usize>(value: &str) -> anyhow::Result<[u8; N]> {
    anyhow::ensure!(
        value.len() == N * 2 && value.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid hexadecimal identifier"
    );
    let mut result = [0; N];
    for (index, slot) in result.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)?;
    }
    Ok(result)
}
fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
