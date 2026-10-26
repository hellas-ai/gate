//! Explicit approvals and assertion counters use the SDK's authoritative store.
use crate::dto::{TrustInput, TrustRecord};
use hellas_rpc::{Assurance, ContentId};
use hellas_sdk::trust_policy::{AppleApproval, ApprovalRecord, TrustPolicy, TrustPolicyStore};
use std::{path::Path, sync::Arc};

pub fn store(directory: &Path) -> TrustPolicyStore {
    TrustPolicyStore::new(directory.join("trust"))
}
pub fn list(directory: &Path) -> anyhow::Result<Vec<TrustRecord>> {
    Ok(store(directory).list()?.into_iter().map(record).collect())
}
fn record(record: ApprovalRecord) -> TrustRecord {
    TrustRecord {
        id: record.id,
        revision: record.revision,
        genesis: record.policy.provider_genesis.to_string(),
        assurance: format!("{:?}", record.policy.assurance),
        environments: record
            .policy
            .environments
            .iter()
            .map(ToString::to_string)
            .collect(),
        source: record.reason,
    }
}
pub fn approve(directory: &Path, input: TrustInput) -> anyhow::Result<TrustRecord> {
    let policy = TrustPolicy {
        provider_genesis: content_id(&input.genesis)?,
        assurance: if input.apple {
            Assurance::AppleAppAttest
        } else {
            Assurance::ProducerSigned
        },
        environments: input
            .environments
            .iter()
            .map(|v| content_id(v))
            .collect::<anyhow::Result<_>>()?,
        apple: if input.apple {
            Some(AppleApproval {
                app_id: input.app_id,
                cd_hashes: input
                    .cd_hashes
                    .iter()
                    .map(|v| fixed(v))
                    .collect::<anyhow::Result<_>>()?,
            })
        } else {
            anyhow::ensure!(
                input.app_id.is_empty() && input.cd_hashes.is_empty(),
                "software approval cannot carry unused Apple pins"
            );
            None
        },
    };
    Ok(record(store(directory).approve(
        &input.id,
        input.expected_revision,
        policy,
        input.reason,
    )?))
}
pub fn resolve(
    directory: &Path,
    id: &str,
    environment: ContentId,
) -> anyhow::Result<hellas_sdk::client::ProviderTrustAnchor> {
    let policy = store(directory).load(id)?;
    // Counter custody does not vary with policy revisions or replacement.
    Ok(store(directory).resolve_environment(
        id,
        policy.policy.assurance,
        environment,
        Arc::new(hellas_sdk::FilesystemAssertionCounterStore::new(
            directory.join("apple-assertion-counters"),
        )),
    )?)
}
pub fn fixed<const N: usize>(text: &str) -> anyhow::Result<[u8; N]> {
    let mut bytes = [0; N];
    hex::decode_to_slice(text, &mut bytes)?;
    Ok(bytes)
}
pub fn content_id(text: &str) -> anyhow::Result<ContentId> {
    Ok(text.parse()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approval_is_explicit_and_keeps_independent_environment_and_genesis_pins() {
        let dir = tempfile::tempdir().unwrap();
        let env = ContentId::from_bytes([2; 32]);
        assert!(resolve(dir.path(), "provider", env).is_err());
        let record = approve(
            dir.path(),
            TrustInput {
                id: "provider".into(),
                expected_revision: 0,
                genesis: hex::encode([1; 32]),
                apple: false,
                environments: vec![hex::encode([2; 32])],
                app_id: String::new(),
                cd_hashes: vec![],
                reason: "Fingerprint compared independently".into(),
            },
        )
        .unwrap();
        assert_eq!(record.revision, 1);
        assert!(resolve(dir.path(), "provider", ContentId::from_bytes([3; 32])).is_err());
        assert_eq!(
            resolve(dir.path(), "provider", env)
                .unwrap()
                .expected_genesis,
            ContentId::from_bytes([1; 32])
        );
        assert_eq!(list(dir.path()).unwrap().len(), 1);
    }
}
