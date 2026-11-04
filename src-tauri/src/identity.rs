//! A person, a node and a local TLS identity have separate keys and custody.
use crate::{
    period::Period,
    vault::{self, Vault},
};
use anyhow::{Context, ensure};
use hellas_auth::{Digest, authority::*};
use hellas_authority::{
    authenticator::Authenticator,
    operations::{OPERATIONS, Scope},
    work_store::grant::{NodeStore, OwnerApproval, PreparedBootstrap},
};
use hellas_sdk::{NodeIdentity, kernel::SoftPasskey};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Arc};

#[derive(Debug)]
pub struct ExistingJournalError;
impl std::fmt::Display for ExistingJournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Existing node journal could not be opened. It may require a compatible Gate/Hellas version; preserve the data and restore the matching version or use a separate data directory")
    }
}
impl std::error::Error for ExistingJournalError {}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installation {
    pub authority: AuthorityId,
    pub root: GrantId,
    pub owner: PrincipalId,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub card: Option<ContactCard>,
    pub prepared: PreparedBootstrap,
    pub credential_id: CredentialId,
    pub owner_scope: GrantSpec,
    pub generation: u64,
    pub certificate: Vec<u8>,
    #[serde(default)]
    pub native_passkey: Option<crate::passkey::NativePasskey>,
}
pub struct Identity {
    pub public: Installation,
    pub node: NodeIdentity,
    pub passkey: Arc<dyn Authenticator>,
    pub credential: CredentialSecret,
}

pub fn random<const N: usize>() -> anyhow::Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| anyhow::anyhow!("secure randomness unavailable"))?;
    Ok(bytes)
}
fn credential_bytes(credential: &CredentialSecret) -> zeroize::Zeroizing<Vec<u8>> {
    let mut bytes = zeroize::Zeroizing::new(credential.id().0.to_vec());
    bytes.extend_from_slice(credential.expose_secret());
    bytes
}
pub fn decode_credential(bytes: &[u8]) -> anyhow::Result<CredentialSecret> {
    ensure!(bytes.len() == 48, "invalid stored credential");
    Ok(CredentialSecret::from_parts(
        CredentialId::try_from(&bytes[..16])?,
        bytes[16..].try_into()?,
    ))
}
pub async fn save_credential(
    vault: &Vault,
    key: &str,
    credential: &CredentialSecret,
) -> anyhow::Result<()> {
    vault.set(key, &credential_bytes(credential)).await
}

impl Identity {
    /// This explicit software-only creation is never a fallback for a native prompt.
    pub async fn create(directory: &Path, vault: &Vault, name: String) -> anyhow::Result<Self> {
        Self::prepare_software(directory, vault, name, random()?).await
    }
    #[cfg(all(test, feature = "chain-tests", target_os = "linux"))]
    pub(crate) async fn fixture(
        directory: &Path,
        vault: &Vault,
        name: String,
        scalar: [u8; 32],
    ) -> anyhow::Result<Self> {
        Self::prepare_software(directory, vault, name, scalar).await
    }
    async fn prepare_software(
        directory: &Path,
        vault: &Vault,
        name: String,
        scalar: [u8; 32],
    ) -> anyhow::Result<Self> {
        ensure!(
            !directory.join("node/authority").try_exists()?,
            "node already initialized"
        );
        ensure!(
            !directory.join("installation.json").try_exists()?,
            "resume the prepared initialization instead"
        );
        let passkey = SoftPasskey::from_secret_scalar(scalar)
            .map_err(|_| anyhow::anyhow!("invalid passkey scalar"))?;
        ensure!(
            !directory.join("native-passkey.json").try_exists()?,
            "resume native passkey setup instead"
        );
        vault.set("owner-software-passkey", &scalar).await?;
        Self::prepare(directory, vault, name, Arc::new(passkey), None).await
    }
    pub async fn create_native(
        directory: &Path,
        vault: &Vault,
        name: String,
    ) -> anyhow::Result<Self> {
        ensure!(
            !directory.join("installation.json").try_exists()?
                && !directory.join("node/authority").try_exists()?,
            "identity already initialized; resume or recover it"
        );
        validate_name(&name)?;
        let path = directory.join("native-passkey.json");
        let native = match vault::read::<crate::passkey::NativePasskey>(&path)? {
            Some(native) => {
                native.validate()?;
                native
            }
            None => {
                let native = crate::passkey::NativePasskey::register(&name).await?;
                vault::write(&path, &native)?;
                native
            }
        };
        Self::prepare(
            directory,
            vault,
            name,
            Arc::new(native.clone()),
            Some(native),
        )
        .await
    }
    async fn prepare(
        directory: &Path,
        vault: &Vault,
        name: String,
        passkey: Arc<dyn Authenticator>,
        native_passkey: Option<crate::passkey::NativePasskey>,
    ) -> anyhow::Result<Self> {
        validate_name(&name)?;
        let owner = PrincipalId::from_public_key(hellas_auth::PublicKey::P256(
            passkey.public_key()?.to_compressed(),
        ))?;
        let node = NodeIdentity::generate();
        let root = GrantId(random()?);
        let authority = AuthorityId(Digest::from_bytes(random()?));
        let now = UnixMillis::now()?;
        let spec = admin_scope(authority, owner, now)?;
        let holder = NodeKey::from_public_key(node.node_key().public_key())?;
        let digest = ownership_digest(
            Ownership::Bootstrap { root, target: None },
            &spec,
            &spec,
            holder,
        );
        let approval = OwnerApproval {
            administrator: spec.clone(),
            credential: spec,
            holder,
            assertion: passkey.assert(digest).await?,
        };
        let (prepared, credential) = approval.prepare(None)?;
        let tls = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
        // All secret material is durable before publishing the prepared verifier.
        let mut secrets = zeroize::Zeroizing::new(node.transport_secret_bytes().to_vec());
        secrets.extend_from_slice(&node.node_secret_bytes());
        vault.set("node-keys", &secrets).await?;
        save_credential(vault, "application-credential", &credential).await?;
        vault
            .set("control-tls-key", &tls.signing_key.serialize_der())
            .await?;
        let public = Installation {
            authority,
            root,
            owner,
            name,
            card: None,
            prepared: prepared.clone(),
            certificate: tls.cert.der().to_vec(),
            credential_id: credential.id(),
            owner_scope: prepared.administrator.clone(),
            generation: 1,
            native_passkey,
        };
        vault::write(&directory.join("installation.json"), &public)?;
        Ok(Self {
            public,
            node,
            passkey,
            credential,
        })
    }
    /// Reading status never requests owner approval. Existing installations can
    /// keep their original embedded card; new exports use a separate durable file.
    pub fn contact_card(&self, directory: &Path) -> anyhow::Result<Option<ContactCard>> {
        let card = vault::read::<ContactCard>(&directory.join("contact-card.json"))?
            .or_else(|| self.public.card.clone());
        if let Some(card) = &card {
            ensure!(
                card.principal() == self.public.owner,
                "contact card differs from owner"
            );
            ensure!(
                card.name() == self.display_name(),
                "contact card name differs from installation"
            );
        }
        Ok(card)
    }
    fn display_name(&self) -> &str {
        if self.public.name.is_empty() {
            self.public.card.as_ref().map_or("", ContactCard::name)
        } else {
            &self.public.name
        }
    }
    pub async fn export_contact_card(&self, directory: &Path) -> anyhow::Result<ContactCard> {
        if let Some(card) = self.contact_card(directory)? {
            return Ok(card);
        }
        let name = self.display_name();
        validate_name(name)?;
        let card = ContactCard::new(
            self.public.owner,
            name.to_owned(),
            self.passkey
                .assert(ContactCard::digest(self.public.owner, name))
                .await?,
        )?;
        // An approval or write failure leaves installation and credentials intact.
        vault::write(&directory.join("contact-card.json"), &card)?;
        Ok(card)
    }
    pub async fn load(directory: &Path, vault: &Vault) -> anyhow::Result<Option<Self>> {
        let Some(public) = vault::read::<Installation>(&directory.join("installation.json"))?
        else {
            ensure!(
                !directory.join("node/authority").try_exists()?,
                "authority exists without installation metadata; owner recovery required"
            );
            return Ok(None);
        };
        let bytes = vault.required("node-keys").await?;
        ensure!(bytes.len() == 64, "invalid node vault entry");
        let node =
            NodeIdentity::from_secret_bytes(bytes[..32].try_into()?, bytes[32..].try_into()?)?;
        let passkey = load_passkey(&public, vault).await?;
        let credential = decode_credential(&vault.required("application-credential").await?)?;
        ensure!(
            credential.id() == public.credential_id,
            "prepared credential ID differs from vault"
        );
        Ok(Some(Self {
            public,
            node,
            passkey,
            credential,
        }))
    }
    pub async fn open_store(&self, directory: &Path) -> anyhow::Result<NodeStore> {
        let path = directory.join("node");
        let network = hellas_authority::model::grant::grant_network();
        let now = UnixMillis::now()?;
        let store = if path.join("authority").try_exists()? {
            NodeStore::open(&path, network, None, now)
                .await
                .context(ExistingJournalError)?
        } else {
            ensure!(
                self.public.generation == 1,
                "initialized authority is missing; owner recovery is required"
            );
            NodeStore::bootstrap_prepared(
                &path,
                network,
                self.public.root,
                self.public.prepared.clone(),
                None,
                now,
            )
            .await?
        };
        let authority = store.state().authority();
        ensure!(
            authority.authority() == self.public.authority,
            "authority differs from installation"
        );
        let issued = authority
            .credential(self.credential.id())
            .context("prepared credential is absent; owner recovery required")?;
        ensure!(
            self.public.generation != 1 || issued == &self.public.prepared.credential,
            "prepared credential differs from committed verifier"
        );
        Ok(store)
    }
}
fn validate_name(name: &str) -> anyhow::Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= MAX_CONTACT_NAME_BYTES
            && !name.chars().any(char::is_control),
        "name must contain 1–64 bytes without control characters"
    );
    Ok(())
}
async fn load_passkey(
    public: &Installation,
    vault: &Vault,
) -> anyhow::Result<Arc<dyn Authenticator>> {
    let passkey: Arc<dyn Authenticator> = if let Some(native) = &public.native_passkey {
        native.validate()?;
        Arc::new(native.clone())
    } else {
        let bytes = vault.required("owner-software-passkey").await?;
        Arc::new(
            SoftPasskey::from_secret_scalar(bytes.as_slice().try_into()?)
                .map_err(|_| anyhow::anyhow!("invalid stored software passkey"))?,
        )
    };
    ensure!(
        passkey.public_key()?.to_compressed() == *public.owner.as_bytes(),
        "stored passkey differs from owner"
    );
    Ok(passkey)
}
fn admin_scope(
    authority: AuthorityId,
    owner: PrincipalId,
    now: UnixMillis,
) -> anyhow::Result<GrantSpec> {
    let mut permissions = std::collections::BTreeSet::new();
    let mut operations = std::collections::BTreeSet::new();
    for op in [
        GRANT_ISSUE,
        GRANT_REVOKE,
        GRANT_ADMINISTER,
        CREDENTIAL_ISSUE,
        CREDENTIAL_REVOKE,
    ] {
        let operation = OperationId::new(op)?;
        operations.insert(operation.clone());
        permissions.insert(Permission {
            operation,
            resource: authority.resource(),
        });
    }
    for op in OPERATIONS {
        operations.insert(OperationId::new(op.name)?);
        if !matches!(op.scope, Scope::Allowance) {
            permissions.insert(op.permission(authority, None)?);
        }
    }
    Ok(GrantSpec {
        authority,
        subject: owner,
        permissions,
        not_before: now,
        expires: Period::Unlimited.expires(now)?,
        delegation_depth: MAX_DELEGATION_DEPTH,
        issuance: Some(IssuanceCeiling {
            operations,
            resources: CeilingResources::Authority,
            max_validity_ms: Period::MAX_MS,
            max_delegation_depth: MAX_DELEGATION_DEPTH,
        }),
    })
}

impl Identity {
    /// Explicit offline owner approval; a lost recovery reply requires another approval.
    pub async fn recover(directory: &Path, vault: &Vault) -> anyhow::Result<Self> {
        let mut public = vault::read::<Installation>(&directory.join("installation.json"))?
            .context("installation metadata is missing")?;
        let passkey = load_passkey(&public, vault).await?;
        let node_bytes = vault.required("node-keys").await?;
        ensure!(node_bytes.len() == 64, "invalid stored node keys");
        let node = NodeIdentity::from_secret_bytes(
            node_bytes[..32].try_into()?,
            node_bytes[32..].try_into()?,
        )?;
        let network = hellas_authority::model::grant::grant_network();
        let inspection = NodeStore::inspect_offline(
            &directory.join("node"),
            network,
            &InspectionQuery::Grant { id: public.root },
        )
        .await?;
        let now = UnixMillis::now()?;
        let administrator = admin_scope(public.authority, public.owner, now)?;
        let holder = NodeKey::from_public_key(node.node_key().public_key())?;
        let assertion = passkey
            .assert(ownership_digest(
                Ownership::Recover {
                    expected: inspection.revision,
                },
                &administrator,
                &administrator,
                holder,
            ))
            .await?;
        let (record, credential) = NodeStore::recover_offline(
            &directory.join("node"),
            network,
            inspection.revision,
            OwnerApproval {
                administrator: administrator.clone(),
                credential: administrator.clone(),
                holder,
                assertion,
            },
            now,
        )
        .await?;
        // The SDK commits recovery before returning its new credential. Never retry it silently.
        save_credential(vault, "application-credential", &credential).await?;
        public.credential_id = credential.id();
        public.owner_scope = administrator;
        public.generation = record.generation;
        vault::write(&directory.join("installation.json"), &public)?;
        let anchor = directory.join("gateway-listener-credential.json");
        if let Some(old) = vault::read::<crate::credentials::PreparedIssue>(&anchor)? {
            vault
                .delete(&crate::credentials::PreparedIssue::key(old.credential.id()))
                .await?;
            std::fs::remove_file(anchor)?;
            hellas_private::sync_directory(directory)?;
        }
        Ok(Self {
            public,
            node,
            passkey,
            credential,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    struct CountingSigner {
        signer: SoftPasskey,
        calls: AtomicUsize,
        cancel: AtomicBool,
    }
    #[async_trait::async_trait]
    impl Authenticator for CountingSigner {
        fn public_key(&self) -> Result<hellas_authority::authenticator::PublicKey, AuthorityError> {
            self.signer.public_key()
        }
        async fn assert(&self, challenge: Digest) -> Result<PasskeyAssertion, AuthorityError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.cancel.load(Ordering::SeqCst) {
                return Err(AuthorityError::Credential);
            }
            self.signer.assert(challenge).await
        }
    }
    #[tokio::test]
    async fn setup_uses_one_assertion_and_contact_export_is_durable_and_cached() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let vault = Vault::fixture(secrets.path());
        vault.set("owner-software-passkey", &[9; 32]).await.unwrap();
        let signer = Arc::new(CountingSigner {
            signer: SoftPasskey::from_secret_scalar([9; 32]).unwrap(),
            calls: AtomicUsize::new(0),
            cancel: AtomicBool::new(false),
        });
        let identity = Identity::prepare(
            directory.path(),
            &vault,
            "Alice".into(),
            signer.clone(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(signer.calls.load(Ordering::SeqCst), 1);
        assert!(identity.contact_card(directory.path()).unwrap().is_none());
        drop(identity.open_store(directory.path()).await.unwrap());
        signer.cancel.store(true, Ordering::SeqCst);
        assert!(
            identity
                .export_contact_card(directory.path())
                .await
                .is_err()
        );
        assert!(identity.contact_card(directory.path()).unwrap().is_none());
        assert!(
            Identity::load(directory.path(), &vault)
                .await
                .unwrap()
                .is_some()
        );
        signer.cancel.store(false, Ordering::SeqCst);
        vault::publication_fault::fail_next_write(directory.path().join("contact-card.json"));
        assert!(
            identity
                .export_contact_card(directory.path())
                .await
                .is_err()
        );
        assert!(identity.contact_card(directory.path()).unwrap().is_none());
        let card = identity
            .export_contact_card(directory.path())
            .await
            .unwrap();
        assert_eq!(signer.calls.load(Ordering::SeqCst), 4);
        assert_eq!(
            identity
                .export_contact_card(directory.path())
                .await
                .unwrap(),
            card
        );
        assert_eq!(signer.calls.load(Ordering::SeqCst), 4);
        let loaded = Identity::load(directory.path(), &vault)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            loaded.contact_card(directory.path()).unwrap(),
            Some(card.clone())
        );
        // An existing embedded signed card needs no migration or new approval.
        let mut old = serde_json::to_value(&identity.public).unwrap();
        old.as_object_mut().unwrap().remove("name");
        old["card"] = serde_json::to_value(&card).unwrap();
        vault::write(&directory.path().join("installation.json"), &old).unwrap();
        std::fs::remove_file(directory.path().join("contact-card.json")).unwrap();
        let legacy = Identity::load(directory.path(), &vault)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            legacy.export_contact_card(directory.path()).await.unwrap(),
            card
        );
    }
    #[tokio::test(flavor = "multi_thread")]
    async fn full_mount_and_reopen_never_request_listener_approval() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let vault = Vault::fixture(secrets.path());
        vault.set("owner-software-passkey", &[9; 32]).await.unwrap();
        let app = crate::state::AppState::prepare(directory.path(), vault.clone()).unwrap();
        let signer = Arc::new(CountingSigner {
            signer: SoftPasskey::from_secret_scalar([9; 32]).unwrap(),
            calls: AtomicUsize::new(0),
            cancel: AtomicBool::new(false),
        });
        let identity = Identity::prepare(
            directory.path(),
            &vault,
            "Alice".into(),
            signer.clone(),
            None,
        )
        .await
        .unwrap();
        app.mount(identity).await.unwrap();
        assert!(app.status().await.unwrap().identity.initialized);
        assert_eq!(
            signer.calls.load(Ordering::SeqCst),
            1,
            "complete setup has only the bootstrap assertion"
        );
        let path = directory.path().join("gateway-listener-credential.json");
        assert!(!path.exists(), "mount must not provision a listener");
        signer.cancel.store(true, Ordering::SeqCst);
        assert!(app.set_gateway(true, None).await.is_err());
        assert!(
            !path.exists(),
            "rejected owner approval must not prepare or issue listener access"
        );
        signer.cancel.store(false, Ordering::SeqCst);

        {
            let local = app.local().await.unwrap();
            let daily = local.session(false).await.unwrap();
            assert!(
                crate::credentials::provision_gateway_anchor(&local, &daily)
                    .await
                    .is_err(),
                "daily sessions cannot issue listener credentials"
            );
            assert!(
                crate::credentials::load_gateway_anchor(&local)
                    .await
                    .is_err()
            );
        }
        // No gateway configuration exists yet: enable fails, but issuance still
        // must use exactly the one owner session, never a second hidden prompt.
        assert!(app.set_gateway(true, None).await.is_err());
        assert_eq!(signer.calls.load(Ordering::SeqCst), 3);
        {
            let local = app.local().await.unwrap();
            crate::credentials::load_gateway_anchor(&local)
                .await
                .unwrap()
                .validate()
                .await
                .unwrap();
            let held = vault::read::<crate::credentials::PreparedIssue>(&path)
                .unwrap()
                .unwrap();
            let permissions = &held.credential.scope().permissions;
            assert_eq!(permissions.len(), 1);
            assert_eq!(
                permissions.iter().next().unwrap().operation.as_str(),
                hellas_authority::node::GATEWAY_INVOKE
            );
            assert_eq!(held.credential.holder(), CredentialHolder::Bearer);
        }
        app.shutdown().await.unwrap();
        drop(app);
        let reopened = crate::state::AppState::prepare(directory.path(), vault.clone()).unwrap();
        let mut identity = Identity::load(directory.path(), &vault)
            .await
            .unwrap()
            .unwrap();
        identity.passkey = signer.clone();
        reopened.mount(identity).await.unwrap();
        assert!(reopened.status().await.unwrap().identity.initialized);
        assert_eq!(
            signer.calls.load(Ordering::SeqCst),
            3,
            "reopen and status must not prompt"
        );
        reopened.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn native_metadata_reloads_without_a_software_secret_and_rejects_a_different_owner() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let vault = Vault::fixture(secrets.path());
        // The software signer supplies fixture proofs only; persistence records
        // the corresponding native public credential, never its private scalar.
        let signer = SoftPasskey::from_secret_scalar([9; 32]).unwrap();
        let native = serde_json::from_value(serde_json::json!({
            "credential_id": [1, 2, 3],
            "public_key": signer.public_key().unwrap().to_compressed().to_vec(),
        }))
        .unwrap();
        let identity = Identity::prepare(
            directory.path(),
            &vault,
            "Alice".into(),
            Arc::new(signer),
            Some(native),
        )
        .await
        .unwrap();
        assert!(vault.required("owner-software-passkey").await.is_err());
        let loaded = Identity::load(directory.path(), &vault)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.public.owner, identity.public.owner);
        assert_eq!(loaded.credential.id(), identity.credential.id());
        let mut public = loaded.public;
        public.native_passkey = None;
        vault::write(&directory.path().join("installation.json"), &public).unwrap();
        assert!(Identity::load(directory.path(), &vault).await.is_err());
        vault.set("owner-software-passkey", &[8; 32]).await.unwrap();
        assert!(Identity::load(directory.path(), &vault).await.is_err());
    }
    #[tokio::test]
    async fn prepared_bootstrap_reconciles_exact_id_after_lost_reply_and_rejects_missing_secret() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let vault = Vault::fixture(secrets.path());
        let identity = Identity::create(directory.path(), &vault, "Alice".into())
            .await
            .unwrap();
        let id = identity.credential.id();
        let owner = identity.public.owner;
        let node = identity.node.node_id();
        let store = identity.open_store(directory.path()).await.unwrap();
        drop(store); // Simulate the committed bootstrap response being lost.
        let identity = Identity::load(directory.path(), &vault)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(identity.credential.id(), id);
        assert_eq!(identity.public.owner, owner);
        assert_eq!(identity.node.node_id(), node);
        let store = identity.open_store(directory.path()).await.unwrap();
        assert_eq!(store.state().authority().credentials().count(), 1);
        drop(store);
        vault.delete("application-credential").await.unwrap();
        assert!(Identity::load(directory.path(), &vault).await.is_err());
        assert!(
            Identity::create(directory.path(), &vault, "Alice".into())
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn software_owner_card_verifies_and_assertions_set_up_never_uv() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let sa = tempfile::tempdir().unwrap();
        let sb = tempfile::tempdir().unwrap();
        let alice = Identity::create(a.path(), &Vault::fixture(sa.path()), "Alice".into())
            .await
            .unwrap();
        let bob = Identity::create(b.path(), &Vault::fixture(sb.path()), "Bob".into())
            .await
            .unwrap();
        assert_ne!(alice.public.owner, bob.public.owner);
        assert_ne!(
            alice.node.node_key().public_key().bytes(),
            alice.public.owner.as_bytes()
        );
        let digest = ContactCard::digest(alice.public.owner, "Alice");
        let assertion = alice.passkey.assert(digest).await.unwrap();
        assert_eq!(assertion.authenticator_data()[32] & 0x05, 0x01);
        assert!(ContactCard::new(alice.public.owner, "Alice".into(), assertion.clone()).is_ok());
        assert!(ContactCard::new(bob.public.owner, "Alice".into(), assertion.clone()).is_err());
        assert!(ContactCard::new(alice.public.owner, "Changed".into(), assertion).is_err());
    }
}
