//! Verified people and explicitly confirmed device handoff; grants stay in Auth.
use crate::{
    control::{self, LocalNode},
    credentials::PreparedIssue,
    dto::*,
    identity, vault,
};
use anyhow::{Context, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use hellas_auth::{Digest, authority::*};
use hellas_authority::model::grant::{admin::*, budget::*};
use hellas_sdk::access_bundle::AccessBundle;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    num::{NonZeroU16, NonZeroU64},
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Contact {
    card: ContactCard,
    label: String,
    devices: BTreeSet<NodeKey>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shared {
    pub intent: Digest,
    pub issue: PreparedIssue,
    pub contact: String,
    pub holder: NodeKey,
    pub allowances: Vec<GrantId>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Imported {
    pub authority: AuthorityId,
    pub authorization: GrantId,
    pub subject: PrincipalId,
    pub holder: NodeKey,
    pub credential_id: CredentialId,
    pub offers: Vec<String>,
    pub expires: UnixMillis,
    pub label: String,
}
impl Imported {
    pub fn id(&self) -> String {
        hex::encode(
            hellas_auth::hash_tuple(
                "hellas.gate.imported-access.v1",
                &[self.authority.0.as_bytes(), &self.credential_id.0],
            )
            .as_bytes(),
        )
    }
    pub fn vault_key(&self) -> String {
        format!(
            "imported-credential-{}-{}",
            self.authority, self.credential_id
        )
    }
}
fn named(directory: &Path, category: &str, id: &str) -> anyhow::Result<PathBuf> {
    let id = id.strip_prefix("p256:").unwrap_or(id);
    ensure!(
        !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid record identifier"
    );
    Ok(directory.join(category).join(format!("{id}.json")))
}
fn list<T: serde::de::DeserializeOwned>(
    directory: &Path,
    category: &str,
) -> anyhow::Result<Vec<T>> {
    let dir = directory.join(category);
    if !dir.try_exists()? {
        return Ok(vec![]);
    }
    let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    ensure!(entries.len() <= 512, "too many local records");
    entries
        .into_iter()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .map(|e| vault::read(&e.path())?.context("record disappeared"))
        .collect()
}
pub fn validate(input: &ContactInput) -> anyhow::Result<()> {
    ensure!(
        input.confirmed_holder,
        "explicitly confirm the recipient device key"
    );
    ensure!(input.label.len() <= 64, "contact label is too long");
    ensure!(input.card.len() <= 8192, "contact card is too large");
    let _card = hellas_authority::model::grant::records::decode_contact_card(
        &STANDARD.decode(&input.card)?,
    )?;
    let _holder = NodeKey::from_public_key(hellas_auth::PublicKey::Secp256k1(
        crate::trust::fixed(&input.holder)?,
    ))?;
    Ok(())
}
pub fn add(directory: &Path, input: ContactInput) -> anyhow::Result<ContactView> {
    ensure!(
        input.confirmed_holder,
        "explicitly confirm the recipient device key"
    );
    ensure!(input.label.len() <= 64, "contact label is too long");
    ensure!(input.card.len() <= 8192, "contact card is too large");
    let card = hellas_authority::model::grant::records::decode_contact_card(
        &STANDARD.decode(&input.card)?,
    )?;
    let holder = NodeKey::from_public_key(hellas_auth::PublicKey::Secp256k1(crate::trust::fixed(
        &input.holder,
    )?))?;
    let id = card.principal().to_string();
    let path = named(directory, "contacts", &id)?;
    let mut contact = vault::read::<Contact>(&path)?.unwrap_or(Contact {
        card: card.clone(),
        label: input.label.clone(),
        devices: BTreeSet::new(),
    });
    ensure!(
        contact.card.principal() == card.principal(),
        "contact identity differs"
    );
    contact.card = card;
    contact.label = input.label;
    contact.devices.insert(holder);
    vault::write(&path, &contact)?;
    Ok(view(contact))
}
fn view(c: Contact) -> ContactView {
    ContactView {
        id: c.card.principal().to_string(),
        fingerprint: c.card.principal().fingerprint(),
        name: c.card.name().into(),
        label: c.label,
        devices: c
            .devices
            .iter()
            .map(|h| hex::encode(h.as_bytes()))
            .collect(),
    }
}
pub fn contacts(directory: &Path) -> anyhow::Result<Vec<ContactView>> {
    Ok(list::<Contact>(directory, "contacts")?
        .into_iter()
        .map(view)
        .collect())
}
pub fn shares(directory: &Path) -> anyhow::Result<Vec<ShareView>> {
    Ok(list::<Shared>(directory, "shares")?
        .into_iter()
        .map(|s| ShareView {
            id: s.issue.id.to_string(),
            contact: s.contact,
            holder: hex::encode(s.holder.as_bytes()),
            label: s.issue.label,
            expires: s.issue.credential.scope().expires.0,
            allowances: s.allowances.iter().map(ToString::to_string).collect(),
            credential: s.issue.credential.id().to_string(),
        })
        .collect())
}
pub fn accesses(directory: &Path) -> anyhow::Result<Vec<AccessView>> {
    Ok(list::<Imported>(directory, "access")?
        .into_iter()
        .map(|i| AccessView {
            id: i.id(),
            label: i.label,
            expires: i.expires.0,
            allowances: i
                .offers
                .iter()
                .filter_map(|s| STANDARD.decode(s).ok())
                .filter_map(|b| {
                    hellas_authority::model::grant::records::SignedOffer::decode(&b, UnixMillis(0))
                        .ok()
                })
                .map(|o| o.offer().grant.id.to_string())
                .collect(),
        })
        .collect())
}
pub fn imported(directory: &Path, id: &str) -> anyhow::Result<Imported> {
    vault::read(&named(directory, "access", id)?)?.context("imported access not found")
}
fn allowance_id(subject: PrincipalId, resource: &str) -> GrantId {
    let hash = hellas_auth::hash_tuple(
        "hellas.gate.allowance.v1",
        &[subject.as_bytes(), resource.as_bytes()],
    );
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash.as_bytes()[..16]);
    GrantId(bytes)
}
async fn budget(session: &control::Session, mut definition: BudgetDef) -> anyhow::Result<()> {
    match session
        .work(WorkControlCommand::InspectBudget {
            node: definition.node,
        })
        .await
    {
        Ok(WorkControlReply::Budget {
            definition: old, ..
        }) => {
            ensure!(old.parent == definition.parent, "budget ancestry conflict");
            definition.revision = old.revision;
            if old == definition {
                return Ok(());
            }
            definition.revision = old.revision.next().context("budget revision overflow")?;
        }
        Err(_) => {}
        _ => anyhow::bail!("invalid budget reply"),
    }
    session
        .work(WorkControlCommand::ConfigureBudget { definition })
        .await?;
    Ok(())
}
pub async fn share(
    directory: &Path,
    local: &LocalNode,
    input: ShareInput,
) -> anyhow::Result<ShareView> {
    let session = local.session(true).await?;
    share_session(directory, local, input, &session).await
}
pub async fn share_session(
    directory: &Path,
    local: &LocalNode,
    mut input: ShareInput,
    session: &control::Session,
) -> anyhow::Result<ShareView> {
    ensure!(
        !input.allowances.is_empty() && input.allowances.len() <= 16,
        "select 1 to 16 explicit upstream allowances"
    );
    ensure!(input.label.len() <= 64, "share label too long");
    let id: GrantId = input.request_id.parse()?;
    input.request_id = id.to_string();
    let path = named(directory, "shares", &id.to_string())?;
    let intent = Digest::hash(&serde_json::to_vec(&input)?);
    if let Some(held) = vault::read::<Shared>(&path)? {
        ensure!(
            held.intent == intent,
            "prepared request ID conflicts with this sharing intent"
        );
        held.issue.reconcile_session(session, &local.vault).await?;
        return shares(directory)?
            .into_iter()
            .find(|s| s.id == id.to_string())
            .context("share metadata missing");
    }
    let contact = vault::read::<Contact>(&named(directory, "contacts", &input.contact)?)?
        .context("verify the contact first")?;
    let holder = NodeKey::from_public_key(hellas_auth::PublicKey::Secp256k1(crate::trust::fixed(
        &input.holder,
    )?))?;
    ensure!(
        contact.devices.contains(&holder),
        "confirm this recipient device before sharing"
    );
    let WorkControlReply::Catalogue(policies) = session.work(WorkControlCommand::Catalogue).await?
    else {
        anyhow::bail!("invalid resource catalogue");
    };
    let mut selected = BTreeSet::new();
    let mut allowances = Vec::new();
    for requested in &input.allowances {
        let policy = policies
            .iter()
            .find(|p| p.name == requested.resource)
            .context("selected resource has not been configured")?;
        ensure!(
            selected.insert(policy.account),
            "select one allowance per upstream account"
        );
        let account = BudgetNode::Account(policy.account);
        let hash = hellas_auth::hash_tuple("hellas.gate.pool.v1", &[policy.account.0.as_bytes()]);
        let mut pool_id = [0; 16];
        pool_id.copy_from_slice(&hash.as_bytes()[..16]);
        let pool = PoolId(pool_id);
        let subject = BudgetNode::Subject {
            pool,
            subject: contact.card.principal(),
        };
        let concurrent =
            NonZeroU16::new(requested.max_in_flight).context("max in flight must be positive")?;
        ensure!(
            requested.requests_per_day > 0,
            "requests per day must be positive"
        );
        for (node, parent, limits, max) in [
            (
                BudgetNode::Pool(pool),
                account,
                vec![],
                NonZeroU16::new(128).context("invalid pool capacity")?,
            ),
            (
                subject,
                BudgetNode::Pool(pool),
                vec![Limit::new(
                    Meter::Requests,
                    Window::Day,
                    requested.requests_per_day,
                )],
                concurrent,
            ),
        ] {
            budget(
                session,
                BudgetDef {
                    node,
                    parent: Some(parent),
                    revision: Revision(1),
                    limits,
                    max_in_flight: max,
                },
            )
            .await?;
        }
        let id = allowance_id(contact.card.principal(), &requested.resource);
        let terms = AllowanceTerms {
            policies: vec![requested.resource.clone()],
            budget: subject,
            max_job_millis: NonZeroU64::new(300_000).context("invalid job duration")?,
            enabled: true,
            allow_account_backed: true,
        };
        let WorkControlReply::Allowances(existing) = session.work(WorkControlCommand::List).await?
        else {
            anyhow::bail!("invalid allowance listing");
        };
        if let Some(old) = existing.iter().find(|g| g.id == id) {
            ensure!(
                old.policies == terms.policies && old.budget == subject,
                "allowance conflicts with selected upstream"
            );
        } else {
            session
                .work(WorkControlCommand::Create { id, terms })
                .await?;
        }
        allowances.push(id);
    }
    let status = session.status().await?;
    let now = UnixMillis::now()?;
    let scope = GrantSpec {
        authority: local.identity.public.authority,
        subject: contact.card.principal(),
        permissions: allowances
            .iter()
            .map(|id| {
                hellas_authority::operations::Operation::named(
                    hellas_authority::grant_service::WORK_EXECUTE,
                )?
                .permission(local.identity.public.authority, Some(*id))
            })
            .collect::<Result<_, _>>()?,
        not_before: now,
        expires: input.period.expires(now)?,
        delegation_depth: 0,
        issuance: None,
    };
    let prepared_path = named(directory, "prepared-issues", &id.to_string())?;
    let issue = session
        .track(
            format!("Prepare credential for share {id}"),
            false,
            PreparedIssue::prepare(
                &prepared_path,
                &local.vault,
                (id, intent),
                scope,
                CredentialHolder::Node(holder),
                status.generation,
                input.label,
            ),
        )
        .await?;
    let shared = Shared {
        intent,
        issue,
        contact: input.contact,
        holder,
        allowances,
    };
    // Publish exact prepared IDs before any authority call; retry is a separate reconcile operation.
    session.track_local(format!("Publish share intent {id}"), || {
        vault::write(&path, &shared)
    })?;
    shared
        .issue
        .reconcile_session(session, &local.vault)
        .await?;
    shares(directory)?
        .into_iter()
        .find(|s| s.id == id.to_string())
        .context("share metadata missing")
}
pub async fn export(directory: &Path, local: &LocalNode, id: &str) -> anyhow::Result<String> {
    let shared =
        vault::read::<Shared>(&named(directory, "shares", id)?)?.context("share not found")?;
    let session = local.session(true).await?;
    shared
        .issue
        .reconcile_session(&session, &local.vault)
        .await?;
    let client = local.client(false).await?;
    let mut offers = Vec::new();
    for id in &shared.allowances {
        let WorkControlReply::Offers(mut held) = control::work(
            &client,
            local.authorization(),
            WorkControlCommand::Offers { id: Some(*id) },
        )
        .await?
        else {
            anyhow::bail!("invalid offers reply");
        };
        ensure!(held.len() == 1, "shared allowance is paused or absent");
        offers.append(&mut held);
    }
    ensure!(
        offers.len() == shared.issue.credential.scope().permissions.len(),
        "offer/permission count mismatch"
    );
    let bundle = AccessBundle {
        offers,
        authority: local.identity.public.authority,
        authorization: shared.issue.id,
        subject: shared.issue.credential.scope().subject,
        holder: shared.holder,
        credential: shared.issue.secret(&local.vault).await?,
        expires: shared.issue.credential.scope().expires,
        label: Some(shared.issue.label),
    };
    Ok(STANDARD.encode(bundle.encode()?))
}
pub async fn import(
    directory: &Path,
    local: &LocalNode,
    encoded: &str,
) -> anyhow::Result<AccessView> {
    ensure!(
        encoded.len() <= hellas_sdk::access_bundle::MAX_ACCESS_BUNDLE_BYTES * 2,
        "access bundle too large"
    );
    let holder = NodeKey::from_public_key(local.identity.node.node_key().public_key())?;
    let bundle = AccessBundle::decode(
        &STANDARD.decode(encoded)?,
        &local.identity.public.owner,
        &holder,
    )?;
    ensure!(bundle.expires > UnixMillis::now()?, "access bundle expired");
    let imported = Imported {
        authority: bundle.authority,
        authorization: bundle.authorization,
        subject: bundle.subject,
        holder: bundle.holder,
        credential_id: bundle.credential.id(),
        offers: bundle
            .offers
            .iter()
            .map(|o| Ok(STANDARD.encode(o.encode()?)))
            .collect::<anyhow::Result<_>>()?,
        expires: bundle.expires,
        label: bundle.label.unwrap_or_default(),
    };
    let path = named(directory, "access", &imported.id())?;
    if let Some(previous) = vault::read::<Imported>(&path)? {
        ensure!(
            previous.authority == imported.authority
                && previous.authorization == imported.authorization
                && previous.subject == imported.subject
                && previous.holder == imported.holder,
            "imported credential ID conflicts with its existing authority binding"
        );
    }
    if let Some(previous) = local.vault.get(&imported.vault_key()).await? {
        let previous = identity::decode_credential(&previous)?;
        ensure!(
            previous.id() == bundle.credential.id()
                && previous.expose_secret() == bundle.credential.expose_secret(),
            "imported credential ID conflicts with its persisted secret"
        );
    } else {
        identity::save_credential(&local.vault, &imported.vault_key(), &bundle.credential).await?;
    }
    vault::write(&path, &imported)?;
    accesses(directory)?
        .into_iter()
        .find(|a| a.id == imported.id())
        .context("imported access metadata missing")
}
pub async fn revoke(directory: &Path, local: &LocalNode, id: &str) -> anyhow::Result<()> {
    let session = local.session(true).await?;
    revoke_session(directory, local, id, &session).await
}
pub async fn revoke_session(
    directory: &Path,
    _local: &LocalNode,
    id: &str,
    session: &control::Session,
) -> anyhow::Result<()> {
    let shared =
        vault::read::<Shared>(&named(directory, "shares", id)?)?.context("share not found")?;
    let view = session
        .inspect(InspectionQuery::Grant {
            id: shared.issue.id,
        })
        .await?;
    let InspectionRecords::Grants { records, .. } = view.records else {
        anyhow::bail!("invalid grant inspection");
    };
    let record = records.first().context("grant missing")?;
    if !record.revoked() {
        session
            .grant(GrantCommand::Revoke {
                authorization: GrantId(session.authorization.as_slice().try_into()?),
                id: shared.issue.id,
                expected: record.revision(),
            })
            .await?;
    }
    let view = session
        .inspect(InspectionQuery::Credential {
            id: shared.issue.credential.id(),
        })
        .await?;
    let InspectionRecords::Credentials { records, .. } = view.records else {
        anyhow::bail!("invalid credential inspection");
    };
    let record = records.first().context("credential missing")?;
    if !record.revoked && record.generation == view.generation {
        session.revoke(record.id, record.revision).await?;
    }
    Ok(())
}
pub async fn pause(local: &LocalNode, id: &str, paused: bool) -> anyhow::Result<()> {
    let session = local.session(false).await?;
    pause_session(id, paused, &session).await
}
pub async fn pause_session(
    id: &str,
    paused: bool,
    session: &control::Session,
) -> anyhow::Result<()> {
    let id: GrantId = id.parse()?;
    let WorkControlReply::Status { allowance, .. } =
        session.work(WorkControlCommand::Inspect { id }).await?
    else {
        anyhow::bail!("invalid allowance status");
    };
    if allowance.enabled != paused {
        return Ok(());
    }
    session
        .work(WorkControlCommand::Revise {
            id,
            expected_revision: allowance.revision,
            terms: AllowanceTerms {
                policies: allowance.policies,
                budget: allowance.budget,
                max_job_millis: NonZeroU64::new(300_000).context("invalid job duration")?,
                enabled: !paused,
                allow_account_backed: true,
            },
        })
        .await?;
    Ok(())
}

pub async fn quota(local: &LocalNode, id: &str) -> anyhow::Result<QuotaView> {
    let session = local.session(false).await?;
    let WorkControlReply::Status { allowance, .. } = control::work(
        &session.client,
        local.authorization(),
        WorkControlCommand::Inspect { id: id.parse()? },
    )
    .await?
    else {
        anyhow::bail!("invalid allowance status");
    };
    let WorkControlReply::Budget { definition, .. } = control::work(
        &session.client,
        local.authorization(),
        WorkControlCommand::InspectBudget {
            node: allowance.budget,
        },
    )
    .await?
    else {
        anyhow::bail!("invalid budget status");
    };
    Ok(QuotaView {
        revision: definition.revision.0,
        requests_per_day: definition
            .limits
            .iter()
            .find(|limit| limit.meter == Meter::Requests && limit.window == Window::Day)
            .map(|limit| limit.amount),
        max_in_flight: definition.max_in_flight.get(),
    })
}

pub async fn set_quota(
    _local: &LocalNode,
    session: &control::Session,
    input: QuotaInput,
) -> anyhow::Result<()> {
    let id: GrantId = input.allowance.parse()?;
    ensure!(input.requests_per_day > 0, "quota must be positive");
    let concurrency =
        NonZeroU16::new(input.max_in_flight).context("concurrency must be positive")?;
    let WorkControlReply::Status { allowance, .. } =
        session.work(WorkControlCommand::Inspect { id }).await?
    else {
        anyhow::bail!("invalid allowance status");
    };
    let WorkControlReply::Budget { mut definition, .. } = session
        .work(WorkControlCommand::InspectBudget {
            node: allowance.budget,
        })
        .await?
    else {
        anyhow::bail!("invalid budget status");
    };
    let current = definition
        .limits
        .iter()
        .find(|l| l.meter == Meter::Requests && l.window == Window::Day)
        .map(|l| l.amount);
    if current == Some(input.requests_per_day) && definition.max_in_flight == concurrency {
        return Ok(());
    }
    ensure!(
        definition.revision.0 == input.expected_revision,
        "quota revision conflict: expected {}, current {}",
        input.expected_revision,
        definition.revision.0
    );
    definition
        .limits
        .retain(|l| l.meter != Meter::Requests || l.window != Window::Day);
    definition.limits.push(Limit::new(
        Meter::Requests,
        Window::Day,
        input.requests_per_day,
    ));
    definition.max_in_flight = concurrency;
    definition.revision = definition
        .revision
        .next()
        .context("budget revision overflow")?;
    session
        .work(WorkControlCommand::ConfigureBudget { definition })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn typed_principal_keeps_hex_filename_and_rejects_path_traversal() {
        let directory = std::path::Path::new("/tmp/gate-test");
        assert_eq!(
            super::named(directory, "contacts", "p256:abcd").unwrap(),
            super::named(directory, "contacts", "abcd").unwrap()
        );
        assert!(super::named(directory, "contacts", "p256:../abcd").is_err());
    }
    use super::*;
    use crate::{state::AppState, vault::Vault};
    use hellas_authority::node::SessionKey;
    #[tokio::test(flavor = "multi_thread")]
    async fn per_upstream_bundles_use_exact_prepared_ids_and_refuse_wrong_person_holder_and_revocation()
     {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let sa = tempfile::tempdir().unwrap();
        let sb = tempfile::tempdir().unwrap();
        let alice = AppState::with_vault(a.path(), Vault::fixture(sa.path()))
            .await
            .unwrap();
        let bob = AppState::with_vault(b.path(), Vault::fixture(sb.path()))
            .await
            .unwrap();
        alice.initialize_software("Alice".into()).await.unwrap();
        bob.initialize_software("Bob".into()).await.unwrap();
        let bob_status = bob.export_contact_card().await.unwrap();
        alice.bind_provider(true).await.unwrap();
        for (i, name) in ["a", "b"].into_iter().enumerate() {
            alice
                .configure_provider(
                    i as u64,
                    ProviderChange::Endpoint {
                        service: name.into(),
                        method: "fetch".into(),
                        hosts: vec!["example.com".into()],
                        allow_private_addresses: false,
                    },
                )
                .await
                .unwrap();
        }
        alice.set_provider(true).await.unwrap();
        for (i, name) in ["a", "b"].into_iter().enumerate() {
            alice
                .configure_resource(
                    (2 + i) as u64,
                    ResourceInput {
                        name: name.into(),
                        service: name.into(),
                        method: "fetch".into(),
                        resource: ResourceKind::Https {
                            tls_roots: vec![],
                            origin: "https://example.com".into(),
                            paths: vec!["/".into()],
                            methods: vec!["GET".into()],
                            credential: None,
                            max_response_bytes: 4096,
                        },
                    },
                )
                .await
                .unwrap();
        }
        let contact = add(
            a.path(),
            ContactInput {
                card: bob_status.identity.contact_card,
                holder: bob_status.identity.node_key.clone(),
                confirmed_holder: true,
                label: "Bob local label".into(),
            },
        )
        .unwrap();
        let local = alice.local().await.unwrap();
        let recipient = bob.local().await.unwrap();
        let input = ShareInput {
            request_id: hex::encode([25; 16]),
            contact: contact.id.clone(),
            holder: bob_status.identity.node_key,
            allowances: vec![AllowanceInput {
                resource: "a".into(),
                requests_per_day: 2,
                max_in_flight: 1,
            }],
            period: crate::period::Period::Week,
            label: "Only A".into(),
        };
        let first = share(a.path(), &local, input.clone()).await.unwrap();
        let again = share(a.path(), &local, input.clone()).await.unwrap();
        assert_eq!(first.credential, again.credential);
        assert_eq!(first.expires, again.expires);
        let encoded = export(a.path(), &local, &first.id).await.unwrap();
        assert!(import(a.path(), &local, &encoded).await.is_err());
        let received = import(b.path(), &recipient, &encoded).await.unwrap();
        assert_eq!(received.allowances.len(), 1);
        let holder =
            NodeKey::from_public_key(recipient.identity.node.node_key().public_key()).unwrap();
        let bundle = AccessBundle::decode(
            &STANDARD.decode(&encoded).unwrap(),
            &recipient.identity.public.owner,
            &holder,
        )
        .unwrap();
        assert_eq!(bundle.offers[0].offer().grant.policies[0].name, "a");
        let shared = vault::read::<Shared>(&named(a.path(), "shares", &first.id).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            shared.issue.credential.scope().permissions.len(),
            bundle.offers.len()
        );
        assert!(
            local
                .coordinator
                .authenticate(&bundle.credential)
                .await
                .is_err()
        );
        assert!(
            local
                .coordinator
                .authenticate_session(
                    &bundle.credential,
                    SessionKey::Node(local.identity.node.node_key())
                )
                .await
                .is_err()
        );
        assert!(
            local
                .coordinator
                .authenticate_session(
                    &bundle.credential,
                    SessionKey::Node(recipient.identity.node.node_key())
                )
                .await
                .is_ok()
        );
        pause(&local, &first.allowances[0], true).await.unwrap();
        assert!(export(a.path(), &local, &first.id).await.is_err());
        pause(&local, &first.allowances[0], false).await.unwrap();
        let wrong =
            NodeKey::from_public_key(hellas_sdk::NodeIdentity::generate().node_key().public_key())
                .unwrap();
        assert!(
            AccessBundle::decode(
                &STANDARD.decode(&encoded).unwrap(),
                &recipient.identity.public.owner,
                &wrong
            )
            .is_err()
        );
        revoke(a.path(), &local, &first.id).await.unwrap();
        assert!(
            local
                .coordinator
                .authenticate_session(
                    &bundle.credential,
                    SessionKey::Node(recipient.identity.node.node_key())
                )
                .await
                .is_err()
        );
        assert!(share(a.path(), &local, input).await.is_err());
        let metadata =
            std::fs::read_to_string(named(b.path(), "access", &received.id).unwrap()).unwrap();
        assert!(!metadata.contains(&hex::encode(bundle.credential.expose_secret())));
        alice.shutdown().await.unwrap();
        bob.shutdown().await.unwrap();
    }
}
