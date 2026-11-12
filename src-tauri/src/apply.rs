//! A desired document plans SDK operations; the authority journals own their state.
use crate::{
    contacts,
    control::{LocalNode, Session},
    dto::*,
    gateway,
    provider_setup::{self, ProviderStep},
};
use anyhow::{Context, ensure};
use hellas_authority::model::grant::admin::{WorkControlCommand, WorkControlReply};

pub struct Plan {
    revision: u64,
    steps: Vec<Step>,
    requires_passkey: bool,
}
enum Step {
    Contact(ContactInput),
    Provider(ProviderStep),
    Resources {
        expected: u64,
        inputs: Vec<ResourceInput>,
        publication: Option<bool>,
    },
    Retire {
        expected: u64,
        name: String,
    },
    Share(ShareInput),
    GatewayKey(GatewayKeyInput),
    RevokeShare(String),
    RevokeKey(String),
    Quota(QuotaInput),
    PaidServe {
        expected: u64,
        input: Option<PaidServeInput>,
    },
    PaidPool(PaidPoolInput),
    ProviderState(bool),
    GatewayConfig(GatewayConfig, bool),
    GatewayState(bool),
}
impl Step {
    fn description(&self) -> String {
        match self {
            Self::Contact(input) => format!(
                "Verify contact and explicitly confirmed holder {}",
                input.holder
            ),
            Self::Provider(step) => match &step.change {
                ProviderChange::Endpoint {
                    service, method, ..
                } => format!("Configure endpoint {service}/{method}"),
                ProviderChange::RemoveEndpoint { service, method } => {
                    format!("Remove endpoint {service}/{method}")
                }
                ProviderChange::AddKey { key } => format!(
                    "Add named key {} on {}/{} from its vault reference",
                    key.key, key.service, key.method
                ),
                ProviderChange::ReplaceKey { key } => format!(
                    "Replace named key {} on {}/{} from its vault reference",
                    key.key, key.service, key.method
                ),
                ProviderChange::RemoveKey {
                    service,
                    method,
                    key,
                } => format!("Remove named key {key} on {service}/{method}"),
            },
            Self::Resources { inputs, .. } => {
                format!(
                    "Activate resource group {} through Work authority",
                    inputs
                        .iter()
                        .map(|input| input.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Self::Retire { name, .. } => {
                format!("Pause allowances for {name} and retire its runtime catalogue entry")
            }
            Self::Share(input) => format!(
                "Issue/reconcile exact share {} to person {} and holder {}, lifetime {:?}",
                input.request_id, input.contact, input.holder, input.period
            ),
            Self::GatewayKey(input) => format!(
                "Issue/reconcile gateway.invoke key {} ({}) for {:?}",
                input.request_id, input.label, input.period
            ),
            Self::RevokeShare(id) => format!("Revoke share grant and device credential {id}"),
            Self::RevokeKey(id) => format!("Revoke gateway key {id}"),
            Self::Quota(input) => format!(
                "Set allowance {} to {} requests/day and concurrency {}",
                input.allowance, input.requests_per_day, input.max_in_flight
            ),
            Self::PaidServe { input, .. } => format!(
                "{} paid Serve through the shared host",
                if input.is_some() {
                    "Configure"
                } else {
                    "Disable and drain"
                }
            ),
            Self::PaidPool(input) => format!("Save typed paid pool {}", input.id),
            Self::ProviderState(enabled) => format!(
                "Set provider {}",
                if *enabled {
                    "running"
                } else {
                    "stopped; drain accepted work"
                }
            ),
            Self::GatewayConfig(_, enabled) => {
                format!("Configure gateway target/archive policy; final enabled={enabled}")
            }
            Self::GatewayState(enabled) => format!(
                "Set gateway {}",
                if *enabled {
                    "running"
                } else {
                    "stopped; drain accepted work"
                }
            ),
        }
    }
}
impl Plan {
    pub fn preview(&self, id: String) -> ApplyPreview {
        ApplyPreview {
            id,
            provider_revision: self.revision,
            steps: self.steps.iter().map(Step::description).collect(),
            requires_passkey: self.requires_passkey,
        }
    }
}
pub async fn plan(local: &LocalNode, mut document: DesiredState) -> anyhow::Result<Plan> {
    for share in &mut document.shares {
        share.request_id = share
            .request_id
            .parse::<hellas_auth::authority::GrantId>()?
            .to_string();
    }
    for key in &mut document.gateway_keys {
        key.request_id = key
            .request_id
            .parse::<hellas_auth::authority::GrantId>()?
            .to_string();
    }
    for id in document
        .revoke_shares
        .iter_mut()
        .chain(document.revoke_gateway_keys.iter_mut())
    {
        *id = id.parse::<hellas_auth::authority::GrantId>()?.to_string();
    }
    ensure!(document.schema == 1, "unsupported desired-state schema");
    ensure!(
        serde_json::to_vec(&document)?.len() <= 4 << 20,
        "document exceeds byte limit"
    );
    ensure!(
        document.contacts.len() <= 256
            && document.shares.len() <= 256
            && document.gateway_keys.len() <= 256,
        "document exceeds record limits"
    );
    for contact in &document.contacts {
        contacts::validate(contact)?;
    }
    for share in &document.shares {
        let _: hellas_auth::authority::GrantId = share.request_id.parse()?;
        ensure!(
            !document.revoke_shares.contains(&share.request_id),
            "one share cannot be both active and revoked"
        );
        ensure!(
            !share.allowances.is_empty() && share.allowances.len() <= 16,
            "select 1 to 16 allowances"
        );
        share
            .period
            .expires(hellas_auth::authority::UnixMillis::now()?)?;
    }
    for key in &document.gateway_keys {
        let _: hellas_auth::authority::GrantId = key.request_id.parse()?;
        ensure!(
            !document.revoke_gateway_keys.contains(&key.request_id),
            "one gateway key cannot be both active and revoked"
        );
        key.period
            .expires(hellas_auth::authority::UnixMillis::now()?)?;
    }
    for quota in &document.quotas {
        let _: hellas_auth::authority::GrantId = quota.allowance.parse()?;
        ensure!(
            quota.requests_per_day > 0 && quota.max_in_flight > 0,
            "quota and concurrency must be positive"
        );
    }
    for id in document
        .revoke_shares
        .iter()
        .chain(document.revoke_gateway_keys.iter())
    {
        let _: hellas_auth::authority::GrantId = id.parse()?;
    }
    let current = local.composition.view()?;
    let paid_change = document
        .paid_serve
        .as_ref()
        .map(|input| input.configuration.clone());
    if let Some(Some(input)) = &paid_change {
        crate::paid::validate_serve(input)?;
    }
    let paid_changed = if let Some(wanted) = &paid_change {
        serde_json::to_vec(wanted)? != serde_json::to_vec(&current.paid)?
    } else {
        false
    };
    let mut pool_ids = std::collections::BTreeSet::new();
    let mut paid_providers = std::collections::BTreeSet::new();
    for pool in &document.paid_pools {
        ensure!(pool_ids.insert(&pool.id), "duplicate paid pool name");
        for provider in &pool.providers {
            ensure!(
                paid_providers.insert(
                    provider
                        .provider
                        .parse::<hellas_sdk::client::ProviderAddress>()?
                        .node
                ),
                "each paid provider must belong to one pool"
            );
        }
        crate::paid::pool_path(&local.directory, &pool.id)?;
        crate::paid::pool_snapshot(&local.directory, pool)?;
    }
    let changes = local
        .composition
        .diff(
            &document.upstreams,
            &document.resources,
            paid_change.as_ref(),
        )
        .await?;
    let retired: Vec<_> = current
        .resources
        .iter()
        .filter(|r| {
            !document
                .resources
                .iter()
                .any(|wanted| wanted.name == r.name)
        })
        .map(|r| r.name.clone())
        .collect();
    let resources = document.resources;
    let changed = paid_changed
        || !changes.is_empty()
        || !retired.is_empty()
        || resources
            .iter()
            .any(|resource| !current.resources.contains(resource));
    if changed {
        ensure!(
            document.expected_provider_revision == current.revision,
            "provider revision conflict: expected {}, current {}",
            document.expected_provider_revision,
            current.revision
        );
    }
    let session = local.session(false).await?;
    let status = session.status().await?;
    let running = status.provider.is_some_and(|p| p.state == 2);
    let active = if running {
        match session.work(WorkControlCommand::Catalogue).await? {
            WorkControlReply::Catalogue(policies) => Some(policies),
            _ => anyhow::bail!("invalid resource catalogue"),
        }
    } else {
        None
    };
    let resources = resources
        .into_iter()
        .map(|resource| {
            let (changed, activated) = if let Some(active) = &active {
                let (_, changed) = local.composition.resource_revision(&resource, active)?;
                (
                    changed,
                    !changed && active.contains(&local.composition.policy(&resource.name)?),
                )
            } else {
                // Stopped means unread, not empty. Reconcile against authority after
                // the explicit start, before assigning subsequent CAS expectations.
                (false, false)
            };
            Ok((resource, changed, activated))
        })
        .collect::<anyhow::Result<Vec<_>>>()?
        .into_iter()
        .filter(|(_, changed, activated)| *changed || !*activated)
        .map(|(resource, changed, _)| (resource, changed))
        .collect::<Vec<_>>();
    let work_needed = paid_change.as_ref().is_some_and(Option::is_some)
        || !retired.is_empty()
        || !resources.is_empty()
        || !document.shares.is_empty()
        || !document.quotas.is_empty();
    ensure!(
        !work_needed || running || document.provider_enabled == Some(true),
        "Work administration requires a mounted provider; enable it explicitly"
    );
    let requires_passkey = document.gateway_enabled == Some(true)
        || (document.gateway.is_some()
            && document.gateway_enabled != Some(false)
            && status
                .gateway
                .as_ref()
                .is_some_and(|gateway| gateway.state == 2))
        || paid_changed && paid_change.as_ref().is_some_and(Option::is_some)
        || changes
            .iter()
            .any(|s| !matches!(s.change, ProviderChange::AddKey { .. }))
        || !document.contacts.is_empty()
        || !document.shares.is_empty()
        || !document.gateway_keys.is_empty()
        || !document.revoke_shares.is_empty()
        || !document.revoke_gateway_keys.is_empty();
    let mut steps = Vec::new();
    steps.extend(document.contacts.into_iter().map(Step::Contact));
    let mut expected = current.revision;
    if paid_changed && current.paid.is_some() {
        steps.push(Step::PaidServe {
            expected,
            input: None,
        });
        expected = expected
            .checked_add(1)
            .context("provider revision overflow")?;
    }
    if !running && document.provider_enabled == Some(true) {
        steps.push(Step::ProviderState(true));
    }
    for name in retired {
        steps.push(Step::Retire { expected, name });
        expected = expected
            .checked_add(1)
            .context("provider revision overflow")?;
    }
    let (before, after): (Vec<_>, Vec<_>) =
        changes.into_iter().partition(|step| !step.after_resources);
    for mut step in before {
        step.expected = expected;
        steps.push(Step::Provider(step));
        expected = expected
            .checked_add(1)
            .context("provider revision overflow")?;
    }
    if !resources.is_empty() {
        let changed = resources.iter().any(|(_, changed)| *changed);
        steps.push(Step::Resources {
            expected,
            inputs: resources.into_iter().map(|(input, _)| input).collect(),
            publication: active.as_ref().map(|_| changed),
        });
        if changed {
            expected = expected
                .checked_add(1)
                .context("provider revision overflow")?;
        }
    }
    for mut step in after {
        step.expected = expected;
        steps.push(Step::Provider(step));
        expected = expected
            .checked_add(1)
            .context("provider revision overflow")?;
    }
    if paid_changed && let Some(Some(input)) = paid_change {
        steps.push(Step::PaidServe {
            expected,
            input: Some(input),
        });
    }
    for pool in document.paid_pools {
        let path = crate::paid::pool_path(&local.directory, &pool.id)?;
        let previous: Option<PaidPoolInput> = crate::vault::read(&path)?;
        if previous.as_ref().map(serde_json::to_vec).transpose()?
            != Some(serde_json::to_vec(&pool)?)
        {
            steps.push(Step::PaidPool(pool));
        }
    }
    steps.extend(document.shares.into_iter().map(Step::Share));
    steps.extend(document.gateway_keys.into_iter().map(Step::GatewayKey));
    steps.extend(document.quotas.into_iter().map(Step::Quota));
    steps.extend(document.revoke_shares.into_iter().map(Step::RevokeShare));
    steps.extend(
        document
            .revoke_gateway_keys
            .into_iter()
            .map(Step::RevokeKey),
    );
    if let Some(config) = document.gateway {
        drop(gateway::validate(local, &config)?);
        let enabled = document
            .gateway_enabled
            .unwrap_or_else(|| status.gateway.as_ref().is_some_and(|g| g.state == 2));
        let previous = local.gateway.get().await;
        if previous.as_ref().map(serde_json::to_vec).transpose()?
            != Some(serde_json::to_vec(&config)?)
        {
            steps.push(Step::GatewayConfig(config, enabled));
        }
    }
    if let Some(enabled) = document.provider_enabled {
        steps.push(Step::ProviderState(enabled));
    }
    if let Some(enabled) = document.gateway_enabled {
        steps.push(Step::GatewayState(enabled));
    }
    Ok(Plan {
        revision: current.revision,
        steps,
        requires_passkey,
    })
}
pub async fn execute(local: &LocalNode, plan: Plan) -> anyhow::Result<ApplyReport> {
    check_revision(local, plan.revision)?;
    let session = local.session(plan.requires_passkey).await?;
    execute_session(local, plan, &session).await
}
fn check_revision(local: &LocalNode, expected: u64) -> anyhow::Result<()> {
    let current = local.composition.view()?.revision;
    ensure!(
        current == expected,
        "provider revision conflict: expected {expected}, current {current}; preview again"
    );
    Ok(())
}
pub async fn execute_session(
    local: &LocalNode,
    plan: Plan,
    session: &Session,
) -> anyhow::Result<ApplyReport> {
    check_revision(local, plan.revision)?;
    session.take_progress()?;
    let labels: Vec<_> = plan.steps.iter().map(Step::description).collect();
    let mut applied = Vec::new();
    let mut unapplied = Vec::new();
    let mut uncertain = Vec::new();
    let mut error = None;
    let mut steps = std::collections::VecDeque::from(plan.steps);
    let mut index = 0;
    while let Some(step) = steps.pop_front() {
        let result: anyhow::Result<()> = async {
            match step {
                Step::Contact(input) => {
                    session.track_local("Publish verified contact/device".into(), || {
                        contacts::add(&local.directory, input)
                    })?;
                }
                Step::Provider(step) => {
                    local
                        .composition
                        .apply(session, step.expected, step.change, step.reference)
                        .await?;
                }
                Step::Resources {
                    expected,
                    inputs,
                    publication,
                } => {
                    check_revision(local, expected)?;
                    if publication.is_none() {
                        let WorkControlReply::Catalogue(active) =
                            session.work(WorkControlCommand::Catalogue).await?
                        else {
                            anyhow::bail!("invalid authority resource catalogue after startup");
                        };
                        let needs_publication = inputs
                            .iter()
                            .map(|input| {
                                local
                                    .composition
                                    .resource_revision(input, &active)
                                    .map(|(_, changed)| changed)
                            })
                            .collect::<anyhow::Result<Vec<_>>>()?
                            .into_iter()
                            .any(|changed| changed);
                        if needs_publication {
                            for step in &mut steps {
                                let revision = match step {
                                    Step::Provider(step) => Some(&mut step.expected),
                                    Step::Resources { expected, .. }
                                    | Step::Retire { expected, .. }
                                    | Step::PaidServe { expected, .. } => Some(expected),
                                    _ => None,
                                };
                                if let Some(revision) = revision {
                                    *revision = revision
                                        .checked_add(1)
                                        .context("provider revision overflow")?;
                                }
                            }
                        }
                    }
                    provider_setup::configure_resources(local, session, expected, inputs).await?;
                }
                Step::Retire { expected, name } => {
                    check_revision(local, expected)?;
                    let WorkControlReply::Allowances(allowances) =
                        session.work(WorkControlCommand::List).await?
                    else {
                        anyhow::bail!("invalid allowance listing");
                    };
                    for allowance in allowances
                        .iter()
                        .filter(|allowance| allowance.enabled && allowance.policies.contains(&name))
                    {
                        contacts::pause_session(&allowance.id.to_string(), true, session).await?;
                    }
                    session.track_local(format!("Retire runtime resource {name}"), || {
                        local.composition.retire_catalogue(expected, &name)
                    })?;
                    session.provider_state(false).await?;
                    session.provider_state(true).await?;
                }
                Step::Share(input) => {
                    contacts::share_session(&local.directory, local, input, session).await?;
                }
                Step::GatewayKey(input) => {
                    gateway::issue_session(local, input, session).await?;
                }
                Step::RevokeShare(id) => {
                    contacts::revoke_session(&local.directory, local, &id, session).await?;
                }
                Step::RevokeKey(id) => {
                    gateway::revoke_session(local, &id, session).await?;
                }
                Step::Quota(input) => {
                    contacts::set_quota(local, session, input).await?;
                }
                Step::PaidServe { expected, input } => {
                    local
                        .composition
                        .configure_paid(local, session, expected, input)
                        .await?;
                }
                Step::PaidPool(input) => {
                    session
                        .track(
                            format!("Publish paid pool {}", input.id),
                            false,
                            local.clients.save_pool(input),
                        )
                        .await?;
                }
                Step::ProviderState(enabled) => {
                    session.provider_state(enabled).await?;
                }
                Step::GatewayConfig(config, enabled) => {
                    gateway::configure(local, session, enabled, Some(config)).await?;
                }
                Step::GatewayState(enabled) => {
                    gateway::configure(local, session, enabled, None).await?;
                }
            }
            Ok(())
        }
        .await;
        let progress = session.take_progress()?;
        if let Err(failure) = result {
            let had_effects = !progress.completed.is_empty();
            applied.extend(
                progress
                    .completed
                    .into_iter()
                    .map(|operation| format!("{}: {operation}", labels[index])),
            );
            if let Some((operation, atomic_rpc)) = progress.pending {
                let refused = atomic_rpc
                    && failure
                        .downcast_ref::<hellas_wire::WireStatus>()
                        .is_some_and(|status| {
                            matches!(
                                status.code(),
                                hellas_wire::WireCode::PermissionDenied
                                    | hellas_wire::WireCode::InvalidArgument
                                    | hellas_wire::WireCode::FailedPrecondition
                            )
                        });
                let detail = format!("{}: {operation}", labels[index]);
                if refused {
                    unapplied.push(detail);
                } else {
                    uncertain.push(detail);
                }
                unapplied.push(format!(
                    "Finish {} (remaining operations were not attempted)",
                    labels[index]
                ));
            } else if had_effects {
                unapplied.push(format!(
                    "Finish {} (stopped after the acknowledged operations above)",
                    labels[index]
                ));
            } else {
                unapplied.push(labels[index].clone());
            }
            unapplied.extend(labels[index + 1..].iter().cloned());
            error = Some(format!("{failure:#}"));
            break;
        }
        applied.push(labels[index].clone());
        index += 1;
    }
    let authority_revision = session.status().await.ok().map(|s| s.authority_revision);
    Ok(ApplyReport {
        applied,
        unapplied,
        uncertain,
        error,
        provider_revision: local.composition.view()?.revision,
        authority_revision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{identity::Identity, vault::Vault};
    use hellas_auth::{Digest, authority::*};
    use hellas_authority::{
        authenticator::{Authenticator, PublicKey},
        node::SessionKey,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Counting {
        inner: Arc<dyn Authenticator>,
        calls: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl Authenticator for Counting {
        fn public_key(&self) -> Result<PublicKey, AuthorityError> {
            self.inner.public_key()
        }
        async fn assert(&self, digest: Digest) -> Result<PasskeyAssertion, AuthorityError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inner.assert(digest).await
        }
    }
    fn document() -> DesiredState {
        DesiredState {
            schema: 1,
            expected_provider_revision: 0,
            upstreams: vec![DesiredUpstream {
                service: "web".into(),
                method: "get".into(),
                hosts: vec!["example.com".into()],
                allow_private_addresses: false,
                keys: vec![DesiredKey {
                    name: "account".into(),
                    origins: vec!["https://example.com".into()],
                    paths: vec!["/".into()],
                    methods: vec!["GET".into()],
                    header: "authorization".into(),
                    prefix: "Bearer ".into(),
                    secret_reference: "declared-source".into(),
                }],
            }],
            resources: vec![],
            contacts: vec![],
            shares: vec![],
            gateway_keys: vec![GatewayKeyInput {
                request_id: hex::encode([110; 16]),
                label: "declarative tool".into(),
                period: crate::period::Period::Week,
            }],
            revoke_shares: vec![],
            revoke_gateway_keys: vec![],
            quotas: vec![],
            provider_enabled: None,
            gateway: None,
            gateway_enabled: None,
            paid_serve: None,
            paid_pools: vec![],
        }
    }
    #[tokio::test(flavor = "multi_thread")]
    async fn real_apply_uses_one_passkey_session_secret_references_and_exact_idempotent_issues() {
        let directory = tempfile::tempdir().unwrap();
        hellas_private::restrict_directory(directory.path()).unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let vault = Vault::fixture(secrets.path());
        let mut identity = Identity::create(directory.path(), &vault, "Owner".into())
            .await
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        identity.passkey = Arc::new(Counting {
            inner: identity.passkey,
            calls: calls.clone(),
        });
        let (local, host) = LocalNode::open(directory.path(), identity, vault.clone())
            .await
            .unwrap();
        calls.store(0, Ordering::SeqCst);
        let desired = document();
        let mut conflicted = desired.clone();
        conflicted.gateway_keys[0].request_id =
            conflicted.gateway_keys[0].request_id.to_uppercase();
        conflicted.revoke_gateway_keys = vec![desired.gateway_keys[0].request_id.clone()];
        assert!(
            plan(&local, conflicted)
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("both active and revoked")
        );
        assert!(
            plan(&local, desired.clone()).await.is_err(),
            "missing declared secret must fail before a passkey session"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        vault
            .set("declared-source", b"fixture-source-secret")
            .await
            .unwrap();
        let prepared = plan(&local, desired.clone()).await.unwrap();
        let preview = prepared.preview("preview".into());
        assert!(preview.requires_passkey);
        assert_eq!(preview.steps.len(), 3);
        let applied = execute(&local, prepared).await.unwrap();
        assert!(applied.error.is_none(), "{:?}", applied.error);
        assert_eq!(applied.applied.len(), 3);
        assert!(applied.unapplied.is_empty() && applied.uncertain.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let view = local.composition.view().unwrap();
        assert_eq!(view.revision, 2);
        assert_eq!(
            view.upstreams[0].keys[0].secret_reference,
            "declared-source"
        );
        let metadata =
            std::fs::read_to_string(directory.path().join("provider-configuration.json")).unwrap();
        assert!(!metadata.contains("fixture-source-secret"));
        let credential: crate::credentials::PreparedIssue = crate::vault::read(
            &directory
                .path()
                .join("prepared-gateway-keys")
                .join(format!("{}.json", desired.gateway_keys[0].request_id)),
        )
        .unwrap()
        .unwrap();
        let secret = credential.secret(&vault).await.unwrap();
        calls.store(0, Ordering::SeqCst);
        let again = execute(&local, plan(&local, desired.clone()).await.unwrap())
            .await
            .unwrap();
        assert!(again.error.is_none());
        assert_eq!(again.provider_revision, applied.provider_revision);
        assert_eq!(again.authority_revision, applied.authority_revision);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        local.coordinator.authenticate(&secret).await.unwrap();
        let mut conflicting = desired;
        conflicting.upstreams[0].hosts.push("other.example".into());
        assert!(
            plan(&local, conflicting)
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("revision conflict")
        );
        host.shutdown().await.unwrap();
        local.clients.shutdown().await.unwrap();
    }
    #[tokio::test(flavor = "multi_thread")]
    async fn real_sdk_refusal_reports_exact_partial_apply_without_atomic_success() {
        let directory = tempfile::tempdir().unwrap();
        hellas_private::restrict_directory(directory.path()).unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let vault = Vault::fixture(secrets.path());
        let identity = Identity::create(directory.path(), &vault, "Owner".into())
            .await
            .unwrap();
        let (local, host) = LocalNode::open(directory.path(), identity, vault)
            .await
            .unwrap();
        let mut desired = document();
        desired.upstreams[0].keys.clear();
        let plan = plan(&local, desired).await.unwrap();
        let mut scope = local.identity.public.owner_scope.clone();
        scope.issuance = None;
        scope.delegation_depth = 0;
        scope.permissions = [
            hellas_authority::node::HOST_STATUS,
            hellas_authority::node::HOST_PROVIDER_CONFIGURE,
        ]
        .into_iter()
        .map(|name| {
            hellas_authority::operations::Operation::named(name)
                .unwrap()
                .permission(scope.authority, None)
                .unwrap()
        })
        .collect();
        let (credential, secret) = Credential::prepare(
            scope,
            CredentialHolder::Node(
                NodeKey::from_public_key(local.identity.node.node_key().public_key()).unwrap(),
            ),
            1,
        )
        .unwrap();
        local
            .session(true)
            .await
            .unwrap()
            .issue(&credential)
            .await
            .unwrap();
        let (tls, server) = hellas_sdk::node_control::profile::local_trust(
            rustls::pki_types::CertificateDer::from(local.identity.public.certificate.clone()),
            "localhost".into(),
        )
        .unwrap();
        let (client, _) = hellas_sdk::node_control::connect_local(
            &local.socket,
            tls,
            server,
            local.identity.public.authority,
            &secret,
            SessionKey::Passkey(local.identity.passkey.as_ref()),
        )
        .await
        .unwrap();
        let limited = Session::new(client, local.authorization());
        let report = execute_session(&local, plan, &limited).await.unwrap();
        assert!(report.applied[0].contains("Configure endpoint"));
        assert!(
            report
                .applied
                .iter()
                .any(|operation| operation.contains("Prepare gateway credential"))
        );
        assert!(
            report
                .applied
                .iter()
                .any(|operation| operation.contains("Publish gateway key intent"))
        );
        assert!(
            report
                .unapplied
                .iter()
                .any(|operation| operation.contains("Auth grant"))
        );
        assert!(
            report
                .unapplied
                .iter()
                .any(|operation| operation.starts_with("Finish "))
        );
        assert!(report.uncertain.is_empty());
        assert!(report.error.is_some());
        assert_eq!(report.provider_revision, 1);
        assert!(report.authority_revision.is_some());
        assert_eq!(local.composition.view().unwrap().upstreams.len(), 1);
        let prepared: crate::credentials::PreparedIssue = crate::vault::read(
            &directory
                .path()
                .join("prepared-gateway-keys")
                .join(format!("{}.json", hex::encode([110; 16]))),
        )
        .unwrap()
        .unwrap();
        assert!(
            local
                .session(true)
                .await
                .unwrap()
                .inspect(InspectionQuery::Credential {
                    id: prepared.credential.id()
                })
                .await
                .is_err()
        );
        host.shutdown().await.unwrap();
        local.clients.shutdown().await.unwrap();
    }
}
