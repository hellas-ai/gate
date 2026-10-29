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
) -> anyhow::Result<hellas_sdk::client::ProviderTrustAnchor> {
    Ok(snapshot(directory, id)?.anchor)
}
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub record: ApprovalRecord,
    pub anchor: hellas_sdk::client::ProviderTrustAnchor,
}
pub(crate) fn snapshot(directory: &Path, id: &str) -> anyhow::Result<Snapshot> {
    let store = store(directory);
    let record = store.load(id)?;
    // Counter custody does not vary with policy revisions or replacement.
    let anchor = store.resolve(
        id,
        record.policy.assurance,
        Arc::new(hellas_sdk::FilesystemAssertionCounterStore::new(
            directory.join("apple-assertion-counters"),
        )),
    )?;
    anyhow::ensure!(
        serde_json::to_vec(&record)? == serde_json::to_vec(&store.load(id)?)?,
        "trust approval changed while resolving; retry the operation"
    );
    Ok(Snapshot { record, anchor })
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
    fn approval_is_explicit_and_keeps_the_independent_enrollment_pin() {
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve(dir.path(), "provider").is_err());
        let record = approve(
            dir.path(),
            TrustInput {
                id: "provider".into(),
                expected_revision: 0,
                genesis: hex::encode([1; 32]),
                apple: false,
                app_id: String::new(),
                cd_hashes: vec![],
                reason: "Fingerprint compared independently".into(),
            },
        )
        .unwrap();
        assert_eq!(record.revision, 1);
        assert_eq!(
            resolve(dir.path(), "provider").unwrap().expected_genesis,
            ContentId::from_bytes([1; 32])
        );
        assert_eq!(list(dir.path()).unwrap().len(), 1);
    }
    #[test]
    fn resolved_policies_keep_counter_high_water_across_replacement_and_fail_closed_on_corruption()
    {
        use hellas_attestation::AttestationError;
        let directory = tempfile::tempdir().unwrap();
        let approval = |revision, hash| TrustInput {
            id: "apple".into(),
            expected_revision: revision,
            genesis: ContentId::from_bytes([1; 32]).to_string(),
            apple: true,
            app_id: "TEAM.test.gate".into(),
            cd_hashes: vec![hex::encode([hash; 32])],
            reason: "Independently checked signed application".into(),
        };
        approve(directory.path(), approval(0, 3)).unwrap();
        let original = resolve(directory.path(), "apple")
            .unwrap()
            .apple_app_attest
            .unwrap();
        let key = [7; 33];
        original.counter_store.advance(&key, 7).unwrap();
        approve(directory.path(), approval(1, 4)).unwrap();
        let replacement = resolve(directory.path(), "apple")
            .unwrap()
            .apple_app_attest
            .unwrap();
        assert_eq!(replacement.allowed_cd_hashes, vec![[4; 32]]);
        assert_eq!(
            replacement.counter_store.advance(&key, 7),
            Err(AttestationError::Counter)
        );
        assert_eq!(
            replacement.counter_store.advance(&key, 6),
            Err(AttestationError::Counter)
        );
        replacement.counter_store.advance(&key, 8).unwrap();
        let path = directory
            .path()
            .join("apple-assertion-counters")
            .join(format!("{}.counter", hex::encode(key)));
        std::fs::write(path, [0, 1]).unwrap();
        assert_eq!(
            resolve(directory.path(), "apple")
                .unwrap()
                .apple_app_attest
                .unwrap()
                .counter_store
                .advance(&key, 9),
            Err(AttestationError::State)
        );
    }
}
