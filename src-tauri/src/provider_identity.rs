//! Provider attestation is a fourth key role, independent of the passkey owner.
use crate::{apple::AppleAppAttestRoot, control::LocalNode, vault};
use anyhow::ensure;
use hellas_attestation::{AttestationError, RootProver};
use hellas_auth::{Digest, ProducerSigningKey, PublicKey};
use hellas_rpc::{
    PlatformCredential, PlatformEnrollment, ProviderEnrollmentBundle, ProviderGenesisStatement,
    RootKind, RootProof, SignedProviderGenesis,
};
use std::{path::Path, sync::Arc};

pub enum ProviderRoot {
    Software(ProducerSigningKey),
    Apple(Arc<AppleAppAttestRoot>),
}
impl RootProver for ProviderRoot {
    async fn prove_statement(&self, bytes: &[u8]) -> Result<RootProof, AttestationError> {
        match self {
            Self::Software(key) => Ok(RootProof::Software(
                key.sign_digest(Digest::hash(bytes))
                    .map_err(|e| AttestationError::Platform(e.to_string()))?,
            )),
            Self::Apple(root) => root.prove_statement(bytes).await,
        }
    }
    async fn prove_open_binding(&self, digest: Digest) -> Result<RootProof, AttestationError> {
        match self {
            Self::Software(key) => Ok(RootProof::Software(
                key.sign_digest(digest)
                    .map_err(|e| AttestationError::Platform(e.to_string()))?,
            )),
            Self::Apple(root) => root.prove_open_binding(digest).await,
        }
    }
}
pub struct ProviderIdentity {
    pub enrollment: ProviderEnrollmentBundle,
    pub root: Arc<ProviderRoot>,
}
impl ProviderIdentity {
    pub async fn load_or_create(
        directory: &Path,
        local: &LocalNode,
        software: bool,
    ) -> anyhow::Result<Self> {
        if let Some(record) = vault::read::<String>(&directory.join("provider-enrollment.json"))? {
            use base64::Engine;
            let enrollment = ProviderEnrollmentBundle::from_canonical_bytes(
                &base64::engine::general_purpose::STANDARD.decode(record)?,
            )?;
            hellas_sdk::work::model::grant::records::verify_provider(&enrollment)?;
            ensure!(
                enrollment.genesis.statement.producer_public_key
                    == local.identity.node.node_key().public_key()
                    && enrollment.genesis.statement.transport_public_key
                        == PublicKey::Ed25519(*local.identity.node.node_id().as_bytes()),
                "provider enrollment differs from node"
            );
            let root = match &enrollment.platform {
                PlatformEnrollment::Absent => {
                    ensure!(
                        software,
                        "software provider enrollment cannot claim Apple assurance"
                    );
                    let bytes = local.vault.required("provider-software-root").await?;
                    ProviderRoot::Software(ProducerSigningKey::from_secret_bytes(
                        bytes.as_slice().try_into()?,
                    )?)
                }
                PlatformEnrollment::AppleAppAttest(platform) => {
                    ensure!(!software, "provider is already Apple-attested");
                    let key = String::from_utf8(
                        local
                            .vault
                            .required("provider-apple-reference")
                            .await?
                            .to_vec(),
                    )?;
                    let credential = hellas_attestation::AppleCredential {
                        attestation: platform.attestation_object.clone(),
                        client_data_hash: platform.client_data_hash,
                    };
                    ProviderRoot::Apple(Arc::new(AppleAppAttestRoot::load(
                        key,
                        credential.content_id(),
                    )))
                }
            };
            return Ok(Self {
                enrollment,
                root: Arc::new(root),
            });
        }
        let nonce = crate::identity::random()?;
        let (root, statement, platform) = if software {
            let root = ProducerSigningKey::generate();
            local
                .vault
                .set("provider-software-root", &root.to_secret_bytes())
                .await?;
            let statement = ProviderGenesisStatement {
                root_kind: RootKind::Software,
                root_public_key: root.public_key(),
                producer_public_key: local.identity.node.node_key().public_key(),
                transport_public_key: PublicKey::Ed25519(*local.identity.node.node_id().as_bytes()),
                platform_credential: PlatformCredential::Absent,
                installation_nonce: nonce,
            };
            (
                ProviderRoot::Software(root),
                statement,
                PlatformEnrollment::Absent,
            )
        } else {
            let mut encoder = hellas_rpc::DagCborEncoder::new();
            encoder.array(2);
            encoder.str("hellas.apple.app-attest.enrollment.v1");
            encoder.bytes(&nonce);
            let bytes = encoder.into_bytes();
            let (root, credential) =
                tokio::task::spawn_blocking(move || AppleAppAttestRoot::create(&bytes)).await??;
            let validation_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            let identity = hellas_attestation::apple_credential_identity(&credential.attestation)?;
            let registered = hellas_attestation::register_apple(
                &credential,
                identity.rp_id_hash,
                hellas_attestation::apple_app_attest_root_ca(),
                hellas_attestation::AnchorTime(validation_time),
            )?;
            local
                .vault
                .set("provider-apple-reference", root.key().as_bytes())
                .await?;
            let statement = ProviderGenesisStatement {
                root_kind: RootKind::SecureEnclave,
                root_public_key: PublicKey::P256(registered.public_key),
                producer_public_key: local.identity.node.node_key().public_key(),
                transport_public_key: PublicKey::Ed25519(*local.identity.node.node_id().as_bytes()),
                platform_credential: PlatformCredential::Registered(credential.content_id()),
                installation_nonce: nonce,
            };
            let platform =
                PlatformEnrollment::AppleAppAttest(hellas_rpc::AppleAppAttestEnrollment {
                    attestation_object: credential.attestation,
                    client_data_hash: credential.client_data_hash,
                    validation_time,
                });
            (ProviderRoot::Apple(Arc::new(root)), statement, platform)
        };
        let enrollment = ProviderEnrollmentBundle {
            genesis: SignedProviderGenesis {
                root_proof: root.prove_statement(&statement.canonical_bytes()).await?,
                statement,
            },
            platform,
        };
        hellas_sdk::work::model::grant::records::verify_provider(&enrollment)?;
        use base64::Engine;
        vault::write(
            &directory.join("provider-enrollment.json"),
            &base64::engine::general_purpose::STANDARD.encode(enrollment.canonical_bytes()),
        )?;
        // Binding is performed by the ordinary owner passkey session by the caller.
        Ok(Self {
            enrollment,
            root: Arc::new(root),
        })
    }
}
