//! HTTP listeners and routes live in the daemon document.
use crate::{control::LocalNode, credentials::PreparedIssue, dto::*, vault};
use anyhow::{Context, ensure};
use hellas_auth::{Digest, authority::*};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub(crate) fn validate(local: &LocalNode, config: &GatewayConfig) -> anyhow::Result<()> {
    validate_presentation(
        !config.http_routes.is_empty(),
        local.clients.presentation(local, &config.client)?,
    )?;
    let mut keys = std::collections::BTreeSet::new();
    for route in &config.http_routes {
        if !keys.insert((&route.path, &route.method)) {
            return Err(hellas_gateway::GatewayError::Config(
                hellas_gateway::GatewayConfigError::DuplicateRoute,
            )
            .into());
        }
        http_route(route).validate()?;
    }
    Ok(())
}
fn http_route(route: &HttpRouteInput) -> hellas_gateway::HttpRoute {
    hellas_gateway::HttpRoute {
        path: route.path.clone(),
        method: route.method.clone(),
        url: route.url.clone(),
        credential: route.credential.clone(),
        tls: hellas_rpc::http_fetch::HttpTls {
            roots: if route.tls_roots.is_empty() {
                hellas_rpc::http_fetch::HttpTrustRoots::WebPki
            } else {
                hellas_rpc::http_fetch::HttpTrustRoots::Certificates {
                    der_base64: route.tls_roots.clone(),
                }
            },
            spki_sha256: vec![],
        },
        headers: vec![],
        max_response_bytes: route.max_response_bytes,
        models: None,
    }
}
fn validate_presentation(http: bool, presentation: FetchPresentation) -> anyhow::Result<()> {
    if http {
        ensure!(
            matches!(presentation, FetchPresentation::Http),
            "HTTP gateway requires an HTTP resource"
        );
    } else {
        ensure!(
            matches!(presentation, FetchPresentation::Responses),
            "Responses gateway requires a Responses contract"
        );
    }
    Ok(())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NamedKey {
    intent: Digest,
    issue: PreparedIssue,
    retirement_requested: bool,
}
fn path(directory: &Path, id: GrantId) -> PathBuf {
    directory.join("gateway-keys").join(format!("{id}.json"))
}

pub async fn issue(
    local: &LocalNode,
    input: GatewayKeyInput,
) -> anyhow::Result<(GatewayKeyView, String)> {
    let session = local.session(true).await?;
    issue_session(local, input, &session).await
}
pub async fn issue_session(
    local: &LocalNode,
    mut input: GatewayKeyInput,
    session: &crate::control::Session,
) -> anyhow::Result<(GatewayKeyView, String)> {
    ensure!(
        !input.label.trim().is_empty() && input.label.len() <= 64,
        "name this gateway key"
    );
    let id: GrantId = input.request_id.parse()?;
    input.request_id = id.to_string();
    let path = path(&local.directory, id);
    let intent = Digest::hash(&serde_json::to_vec(&input)?);
    let named = match vault::read::<NamedKey>(&path)? {
        Some(key) => {
            ensure!(
                key.intent == intent,
                "prepared key ID conflicts with this intent"
            );
            ensure!(
                !key.retirement_requested,
                "this gateway key was retired; choose a new request ID"
            );
            key
        }
        None => {
            let now = UnixMillis::now()?;
            let scope = GrantSpec {
                authority: local.identity.public.authority,
                subject: local.identity.public.owner,
                permissions: [hellas_authority::operations::Operation::named(
                    hellas_authority::node::GATEWAY_INVOKE,
                )?
                .permission(local.identity.public.authority, None)?]
                .into(),
                not_before: now,
                expires: input
                    .period
                    .expires_within(now, local.identity.public.owner_scope.expires)?,
                delegation_depth: 0,
                issuance: None,
            };
            let generation = session.status().await?.generation;
            let issue = session
                .track(
                    format!("Prepare gateway credential {id}"),
                    false,
                    PreparedIssue::prepare(
                        &local
                            .directory
                            .join("prepared-gateway-keys")
                            .join(format!("{id}.json")),
                        &local.vault,
                        (id, intent),
                        scope,
                        CredentialHolder::Bearer,
                        generation,
                        input.label,
                    ),
                )
                .await?;
            let key = NamedKey {
                intent,
                issue,
                retirement_requested: false,
            };
            session.track_local(format!("Publish gateway key intent {id}"), || {
                vault::write(&path, &key)
            })?;
            key
        }
    };
    named.issue.reconcile_session(session, &local.vault).await?;
    let secret = named.issue.secret(&local.vault).await?;
    let bearer = format!("{}{}", secret.id(), hex::encode(secret.expose_secret()));
    Ok((
        GatewayKeyView {
            id: id.to_string(),
            label: named.issue.label,
            expires: named.issue.credential.scope().expires.0,
            state: GatewayKeyState::Active,
        },
        bearer,
    ))
}
struct KeySnapshot {
    generation: u64,
    credentials: std::collections::BTreeMap<CredentialId, CredentialMetadata>,
    grants: std::collections::BTreeMap<GrantId, Grant>,
}
async fn snapshot(session: &crate::control::Session) -> anyhow::Result<KeySnapshot> {
    session
        .inspect(InspectionQuery::Grant {
            id: GrantId(session.authorization.as_slice().try_into()?),
        })
        .await?;
    let mut credentials = std::collections::BTreeMap::new();
    let mut grants = std::collections::BTreeMap::new();
    let mut stamp = None;
    let mut check = |view: &AuthorityInspection| -> anyhow::Result<()> {
        let current = (view.generation, view.revision);
        if let Some(previous) = stamp {
            ensure!(
                current == previous,
                "authority changed while listing keys; retry"
            );
        } else {
            stamp = Some(current);
        }
        Ok(())
    };
    let mut after = None;
    loop {
        let view = session
            .inspect(InspectionQuery::Credentials { after })
            .await?;
        check(&view)?;
        let InspectionRecords::Credentials { records, next } = view.records else {
            anyhow::bail!("invalid credential listing");
        };
        credentials.extend(records.into_iter().map(|record| (record.id, record)));
        ensure!(
            credentials.len() <= 512,
            "credential listing exceeds bounds"
        );
        if let Some(next) = next {
            ensure!(
                after.is_none_or(|previous| next > previous),
                "credential cursor did not advance"
            );
            after = Some(next);
        } else {
            break;
        }
    }
    let mut after = None;
    loop {
        let view = session.inspect(InspectionQuery::Grants { after }).await?;
        check(&view)?;
        let InspectionRecords::Grants { records, next } = view.records else {
            anyhow::bail!("invalid grant listing");
        };
        grants.extend(records.into_iter().map(|record| (record.id(), record)));
        ensure!(grants.len() <= 512, "grant listing exceeds bounds");
        if let Some(next) = next {
            ensure!(
                after.is_none_or(|previous| next > previous),
                "grant cursor did not advance"
            );
            after = Some(next);
        } else {
            break;
        }
    }
    let generation = stamp.context("authority listing is absent")?.0;
    Ok(KeySnapshot {
        generation,
        credentials,
        grants,
    })
}
fn key_view(key: &NamedKey, snapshot: &KeySnapshot) -> anyhow::Result<GatewayKeyView> {
    let credential = snapshot.credentials.get(&key.issue.credential.id());
    if let Some(record) = credential {
        ensure!(
            record.scope == *key.issue.credential.scope()
                && record.holder == key.issue.credential.holder(),
            "gateway credential differs from its prepared intent"
        );
    }
    let credential_live = credential
        .is_some_and(|record| !record.revoked && record.generation == snapshot.generation);
    let grant_live = snapshot
        .grants
        .get(&key.issue.id)
        .is_some_and(|grant| !grant.revoked());
    let state = if key.issue.generation != snapshot.generation {
        GatewayKeyState::Revoked
    } else if key.retirement_requested {
        if credential_live || grant_live {
            GatewayKeyState::Revoking
        } else {
            GatewayKeyState::Revoked
        }
    } else if credential.is_some_and(|record| record.revoked) {
        GatewayKeyState::Revoked
    } else if key.issue.credential.scope().expires <= UnixMillis::now()? {
        GatewayKeyState::Expired
    } else if credential.is_none() {
        GatewayKeyState::Pending
    } else {
        GatewayKeyState::Active
    };
    Ok(GatewayKeyView {
        id: key.issue.id.to_string(),
        label: key.issue.label.clone(),
        expires: key.issue.credential.scope().expires.0,
        state,
    })
}
pub async fn list(local: &LocalNode) -> anyhow::Result<Vec<GatewayKeyView>> {
    let directory = local.directory.join("gateway-keys");
    if !directory.try_exists()? {
        return Ok(vec![]);
    }
    let session = local.session(false).await?;
    let snapshot = snapshot(&session).await?;
    let mut keys = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        ensure!(keys.len() < 512, "gateway key listing exceeds bounds");
        let key =
            vault::read::<NamedKey>(&entry?.path())?.context("gateway key metadata absent")?;
        keys.push(key_view(&key, &snapshot)?);
    }
    keys.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(keys)
}
pub async fn reconcile(local: &LocalNode, id: &str) -> anyhow::Result<(GatewayKeyView, String)> {
    let session = local.session(true).await?;
    let key = vault::read::<NamedKey>(&path(&local.directory, id.parse()?))?
        .context("gateway key not found")?;
    ensure!(
        !key.retirement_requested,
        "this key was retired; issue a new key instead"
    );
    ensure!(
        key.issue.generation == session.status().await?.generation,
        "owner recovery invalidated this preparation; issue a new key"
    );
    let secret = key.issue.secret(&local.vault).await?;
    key.issue.reconcile_session(&session, &local.vault).await?;
    let view = key_view(&key, &snapshot(&session).await?)?;
    Ok((
        view,
        format!("{}{}", secret.id(), hex::encode(secret.expose_secret())),
    ))
}
pub async fn revoke(local: &LocalNode, id: &str) -> anyhow::Result<()> {
    let session = local.session(true).await?;
    revoke_session(local, id, &session).await
}
pub async fn revoke_session(
    local: &LocalNode,
    id: &str,
    session: &crate::control::Session,
) -> anyhow::Result<()> {
    let path = path(&local.directory, id.parse()?);
    let mut key = vault::read::<NamedKey>(&path)?.context("gateway key not found")?;
    let snapshot = snapshot(session).await?;
    // The durable retirement intent prevents a pruned credential from ever being
    // mistaken for an unissued preparation and reactivated with its old secret.
    key.retirement_requested = true;
    session.track_local(
        format!("Retire gateway key intent {}", key.issue.id),
        || vault::write(&path, &key),
    )?;
    if let Some(record) = snapshot.credentials.get(&key.issue.credential.id())
        && !record.revoked
        && record.generation == snapshot.generation
    {
        session.revoke(record.id, record.revision).await?;
    }
    if let Some(grant) = snapshot.grants.get(&key.issue.id)
        && !grant.revoked()
    {
        session
            .grant(GrantCommand::Revoke {
                authorization: GrantId(session.authorization.as_slice().try_into()?),
                id: key.issue.id,
                expected: grant.revision(),
            })
            .await?;
    }
    session
        .track(
            format!(
                "Remove retired credential custody {}",
                key.issue.credential.id()
            ),
            false,
            local
                .vault
                .delete(&PreparedIssue::key(key.issue.credential.id())),
        )
        .await?;
    Ok(())
}

pub async fn rotate(
    local: &LocalNode,
    old: &str,
    input: GatewayKeyInput,
) -> anyhow::Result<(GatewayKeyView, String)> {
    let session = local.session(true).await?;
    let made = issue_session(local, input, &session).await?;
    revoke_session(local, old, &session)
        .await
        .context("new gateway key issued; old key revocation failed; inspect key status")?;
    Ok(made)
}

pub async fn configure(
    local: &LocalNode,
    session: &crate::control::Session,
    enabled: bool,
    config: Option<GatewayConfig>,
) -> anyhow::Result<()> {
    use hellas_sdk::daemon_config::Listener;
    let mut document = session.view().await?.document;
    if let Some(config) = config {
        validate(local, &config)?;
        let (backend, service, method, presentation) =
            local.clients.configuration(local, &config.client).await?;
        document.backends.insert("gateway".into(), backend);
        document
            .routes
            .retain(|route| !route.backends.iter().any(|id| id == "gateway"));
        if presentation == FetchPresentation::Responses {
            document.routes.push(hellas_gateway::Route {
                backends: vec!["gateway".into()],
                presentation: hellas_gateway::Presentation::Responses {
                    service,
                    method,
                    models: vec![],
                    overrides: Default::default(),
                },
            });
        } else {
            for route in config.http_routes {
                let target = hellas_gateway::HttpTarget {
                    service: service.clone(),
                    method: method.clone(),
                    route: http_route(&route),
                };
                document.routes.push(hellas_gateway::Route {
                    backends: vec!["gateway".into()],
                    presentation: hellas_gateway::Presentation::Http {
                        targets: [("gateway".into(), target)].into(),
                    },
                });
            }
        }
        if enabled {
            document.listeners.insert(
                "gateway".into(),
                Listener::Http {
                    bind: std::net::SocketAddr::from(([127, 0, 0, 1], config.port.unwrap_or(0))),
                    zdr: config.zdr,
                },
            );
        }
    } else if enabled {
        ensure!(
            document.listeners.contains_key("gateway"),
            "configure the gateway listener before starting"
        );
    }
    if !enabled {
        document.listeners.remove("gateway");
    }
    document.validate().map_err(anyhow::Error::msg)?;
    local.clients.configure_gateway(session, document).await?;
    Ok(())
}
