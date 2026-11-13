//! Preview one daemon configuration, alongside explicit authority operations.
use crate::{
    contacts,
    control::{LocalNode, Session},
    dto::*,
    gateway, provider_setup,
};
use anyhow::{Context, ensure};
use hellas_sdk::daemon_config::{BackendKind, Document};

pub struct Plan {
    revision: u64,
    steps: Vec<Step>,
    requires_passkey: bool,
}
enum Step {
    Configure(Document),
    Contact(ContactInput),
    Share(ShareInput),
    GatewayKey(GatewayKeyInput),
    RevokeShare(String),
    RevokeKey(String),
    Quota(QuotaInput),
    PaidServe(PaidServeInput),
    PaidPool(PaidPoolInput),
    GatewayConfig(GatewayConfig, bool),
    GatewayState(bool),
}
impl Step {
    fn description(&self) -> String {
        match self {
            Self::Configure(_) => {
                "Configure backends, resources and provider listener atomically".into()
            }
            Self::Contact(input) => format!(
                "Verify contact and explicitly confirmed holder {}",
                input.holder
            ),
            Self::Share(input) => format!(
                "Issue/reconcile exact share {} to {} and holder {}",
                input.request_id, input.contact, input.holder
            ),
            Self::GatewayKey(input) => format!(
                "Issue/reconcile gateway key {} ({})",
                input.request_id, input.label
            ),
            Self::RevokeShare(id) => format!("Revoke share grant and device credential {id}"),
            Self::RevokeKey(id) => format!("Revoke gateway key {id}"),
            Self::Quota(input) => format!(
                "Set allowance {} to {} requests/day and concurrency {}",
                input.allowance, input.requests_per_day, input.max_in_flight
            ),
            Self::PaidServe(_) => "Approve standing paid terms".into(),
            Self::PaidPool(input) => format!("Save typed paid pool {}", input.id),
            Self::GatewayConfig(_, enabled) => format!("Configure gateway; enabled={enabled}"),
            Self::GatewayState(enabled) => format!("Set gateway enabled={enabled}"),
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
    let session = local.session(false).await?;
    let snapshot = session.view().await?;
    let revision = snapshot.revision.0;
    ensure!(
        document.expected_provider_revision == revision,
        "provider revision conflict"
    );
    let mut candidate = snapshot.document.clone();
    candidate
        .backends
        .retain(|_, b| !matches!(b.kind, BackendKind::Fetch { .. }));
    for upstream in document.upstreams {
        let id = provider_setup::route_id(&upstream.service, &upstream.method);
        ensure!(!candidate.backends.contains_key(&id), "duplicate backend");
        let responses = document.resources.iter().any(|r| {
            r.service == upstream.service
                && r.method == upstream.method
                && matches!(r.resource, ResourceKind::OpenAi { .. })
        });
        if responses {
            ensure!(
                upstream.keys.len() == 1,
                "Responses requires one write-only backend key"
            );
            provider_setup::change(
                &mut candidate,
                ProviderChange::OpenAi {
                    service: upstream.service,
                    method: upstream.method,
                    secret: upstream.keys[0].secret.clone(),
                },
            )?;
        } else {
            provider_setup::change(
                &mut candidate,
                ProviderChange::Endpoint {
                    service: upstream.service.clone(),
                    method: upstream.method.clone(),
                    hosts: upstream.hosts,
                    allow_private_addresses: upstream.allow_private_addresses,
                },
            )?;
            let mut keys = std::collections::BTreeSet::new();
            for key in upstream.keys {
                ensure!(keys.insert(key.name.clone()), "duplicate backend key");
                provider_setup::put_key(&mut candidate, &upstream.service, &upstream.method, key)?;
            }
        }
        if let Some(old) = snapshot.document.backends.get(&id) {
            candidate
                .backends
                .get_mut(&id)
                .context("backend was not created")?
                .max_in_flight = old.max_in_flight;
        }
    }
    provider_setup::resources(&mut candidate, document.resources)?;
    if let Some(enabled) = document.provider_enabled {
        provider_setup::enabled(&mut candidate, enabled);
    }
    provider_setup::sync_listener(&mut candidate);
    let mut steps = Vec::new();
    if candidate != snapshot.document {
        steps.push(Step::Configure(candidate));
    }
    steps.extend(document.contacts.into_iter().map(Step::Contact));
    if let Some(input) = document.paid_serve {
        crate::paid::validate_serve(&input)?;
        steps.push(Step::PaidServe(input));
    }
    let mut pools = std::collections::BTreeSet::new();
    let mut providers = std::collections::BTreeSet::new();
    for pool in document.paid_pools {
        for provider in &pool.providers {
            ensure!(
                providers.insert(
                    provider
                        .provider
                        .parse::<hellas_sdk::client::ProviderAddress>()?
                        .node
                ),
                "each paid provider must belong to one pool"
            );
        }
        ensure!(pools.insert(pool.id.clone()), "duplicate paid pool");
        crate::paid::validate_pool(&local.directory, &pool)?;
        steps.push(Step::PaidPool(pool));
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
        gateway::validate(local, &config).await?;
        steps.push(Step::GatewayConfig(
            config,
            document
                .gateway_enabled
                .unwrap_or(!snapshot.http_addresses.is_empty()),
        ));
    } else if let Some(enabled) = document.gateway_enabled {
        steps.push(Step::GatewayState(enabled));
    }
    let requires_passkey = steps
        .iter()
        .any(|step| !matches!(step, Step::Quota(_) | Step::PaidPool(_)));
    Ok(Plan {
        revision,
        steps,
        requires_passkey,
    })
}
pub async fn execute(local: &LocalNode, plan: Plan) -> anyhow::Result<ApplyReport> {
    let session = local.session(plan.requires_passkey).await?;
    execute_session(local, plan, &session).await
}
pub async fn execute_session(
    local: &LocalNode,
    plan: Plan,
    session: &Session,
) -> anyhow::Result<ApplyReport> {
    ensure!(
        session.view().await?.revision.0 == plan.revision,
        "provider revision conflict; preview again"
    );
    session.take_progress()?;
    let labels: Vec<_> = plan.steps.iter().map(Step::description).collect();
    let mut applied = Vec::new();
    let mut unapplied = Vec::new();
    let mut uncertain = Vec::new();
    let mut error = None;
    for (index, step) in plan.steps.into_iter().enumerate() {
        let result: anyhow::Result<()> = async {
            match step {
                Step::Configure(document) => {
                    session.configure(plan.revision, document).await?;
                }
                Step::Contact(input) => {
                    session.track_local("Publish verified contact/device".into(), || {
                        contacts::add(&local.directory, input)
                    })?;
                }
                Step::Share(input) => {
                    contacts::share_session(&local.directory, local, input, session).await?;
                }
                Step::GatewayKey(input) => {
                    gateway::issue_session(local, input, session).await?;
                }
                Step::RevokeShare(id) => {
                    contacts::revoke_session(&local.directory, local, &id, session).await?
                }
                Step::RevokeKey(id) => gateway::revoke_session(local, &id, session).await?,
                Step::Quota(input) => {
                    contacts::set_quota(local, session, input).await?;
                }
                Step::PaidServe(input) => {
                    crate::paid::configure_serve(local, session, input).await?;
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
                Step::GatewayConfig(config, enabled) => {
                    gateway::configure(local, session, enabled, Some(config)).await?
                }
                Step::GatewayState(enabled) => {
                    gateway::configure(local, session, enabled, None).await?
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
    }
    let authority_revision = session.status().await.ok().map(|s| s.authority_revision);
    Ok(ApplyReport {
        applied,
        unapplied,
        uncertain,
        error,
        provider_revision: session.view().await?.revision.0,
        authority_revision,
    })
}
