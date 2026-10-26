//! One SDK gateway factory. Named HTTP keys are ordinary authority credentials.
use crate::{control::LocalNode, credentials::PreparedIssue, dto::*, vault};
use anyhow::{Context, ensure};
use hellas_auth::{Digest, authority::*};
use hellas_sdk::node_control::host::GatewayFactory;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

pub struct Configuration {
    directory: PathBuf,
    config: Mutex<Option<GatewayConfig>>,
}
impl Configuration {
    pub fn open(directory: &Path) -> anyhow::Result<Self> {
        Ok(Self {
            directory: directory.to_owned(),
            config: Mutex::new(vault::read(&directory.join("gateway-configuration.json"))?),
        })
    }
    pub async fn get(&self) -> Option<GatewayConfig> {
        self.config.lock().await.clone()
    }
    pub async fn set(&self, input: GatewayConfig) -> anyhow::Result<()> {
        vault::write(&self.directory.join("gateway-configuration.json"), &input)?;
        *self.config.lock().await = Some(input);
        Ok(())
    }
}
pub fn factory(local: Arc<LocalNode>) -> GatewayFactory {
    Arc::new(move |access| {
        let local = local.clone();
        Box::pin(async move {
            let result: anyhow::Result<hellas_gateway::GatewayHandle> = async {
                let config = local
                    .gateway
                    .get()
                    .await
                    .context("configure a gateway target first")?;
                let target = local.clients.target(&local, &config.client).await?;
                let archive = hellas_gateway::ArchiveOptions {
                    directory: local.directory.join("gateway-archive"),
                    zdr: config.zdr,
                };
                let handle = if config.http_routes.is_empty() {
                    ensure!(
                        matches!(target.presentation, FetchPresentation::Responses),
                        "Responses gateway requires a Responses contract"
                    );
                    hellas_gateway::start_fetch(hellas_gateway::FetchGatewayOptions {
                        access,
                        archive,
                        allow_remote: false,
                        wrap: None,
                        wrap_args: vec![],
                        host: "127.0.0.1".into(),
                        port: config.port,
                        provider: target.provider,
                        service: target.service.clone(),
                        method: target.method.clone(),
                        request_overrides: Default::default(),
                        work: target.backend.clone(),
                    })
                    .await?
                } else {
                    ensure!(
                        matches!(config.client.target, WorkTarget::Paid { .. })
                            && matches!(target.presentation, FetchPresentation::Http),
                        "HTTP gateway requires a paid HTTP pool"
                    );
                    let routes = config
                        .http_routes
                        .into_iter()
                        .map(|r| hellas_gateway::HttpRoute {
                            path: r.path,
                            method: r.method,
                            url: r.url,
                            credential: r.credential,
                            tls: hellas_rpc::http_fetch::HttpTls {
                                roots: if r.tls_roots.is_empty() {
                                    hellas_rpc::http_fetch::HttpTrustRoots::WebPki
                                } else {
                                    hellas_rpc::http_fetch::HttpTrustRoots::Certificates {
                                        der_base64: r.tls_roots,
                                    }
                                },
                                spki_sha256: vec![],
                            },
                            headers: vec![],
                            max_response_bytes: r.max_response_bytes,
                        })
                        .collect();
                    hellas_gateway::start_http(hellas_gateway::HttpGatewayOptions {
                        config: hellas_gateway::HttpGatewayConfig {
                            service: target.service.clone(),
                            method: target.method.clone(),
                            routes,
                            backends: Default::default(),
                            max_in_flight: 4,
                        },
                        paid: target.backend.clone(),
                        archive,
                        host: "127.0.0.1".into(),
                        port: config.port,
                        access,
                        allow_remote: false,
                        wrap: None,
                        wrap_args: vec![],
                    })
                    .await?
                };
                Ok(handle)
            }
            .await;
            result.map_err(anyhow::Error::into_boxed_dyn_error)
        })
    })
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NamedKey {
    intent: Digest,
    issue: PreparedIssue,
}
fn path(directory: &Path, id: &str) -> anyhow::Result<PathBuf> {
    let _: GrantId = id.parse()?;
    Ok(directory.join("gateway-keys").join(format!("{id}.json")))
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
    input: GatewayKeyInput,
    session: &crate::control::Session,
) -> anyhow::Result<(GatewayKeyView, String)> {
    ensure!(
        !input.label.trim().is_empty() && input.label.len() <= 64,
        "name this gateway key"
    );
    let id: GrantId = input.request_id.parse()?;
    let path = path(&local.directory, &input.request_id)?;
    let intent = Digest::hash(&serde_json::to_vec(&input)?);
    let named = match vault::read::<NamedKey>(&path)? {
        Some(key) => {
            ensure!(
                key.intent == intent,
                "prepared key ID conflicts with this intent"
            );
            key
        }
        None => {
            let now = UnixMillis::now()?;
            let scope = GrantSpec {
                authority: local.identity.public.authority,
                subject: local.identity.public.owner,
                permissions: [hellas_sdk::work::operations::Operation::named(
                    hellas_sdk::work::node::GATEWAY_INVOKE,
                )?
                .permission(local.identity.public.authority, None)?]
                .into(),
                not_before: now,
                expires: input
                    .period
                    .expires(now)?
                    .min(local.identity.public.owner_scope.expires),
                delegation_depth: 0,
                issuance: None,
            };
            let generation = session.status().await?.generation;
            let issue = PreparedIssue::prepare(
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
            )
            .await?;
            let key = NamedKey { intent, issue };
            vault::write(&path, &key)?;
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
            revoked: false,
        },
        bearer,
    ))
}
pub async fn list(local: &LocalNode) -> anyhow::Result<Vec<GatewayKeyView>> {
    let directory = local.directory.join("gateway-keys");
    if !directory.try_exists()? {
        return Ok(vec![]);
    }
    let session = local.session(false).await?;
    let generation = session.status().await?.generation;
    let mut keys = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let key =
            vault::read::<NamedKey>(&entry?.path())?.context("gateway key metadata absent")?;
        if key.issue.generation != generation {
            keys.push(GatewayKeyView {
                id: key.issue.id.to_string(),
                label: key.issue.label,
                expires: key.issue.credential.scope().expires.0,
                revoked: true,
            });
            continue;
        }
        let view = session
            .inspect(InspectionQuery::Credential {
                id: key.issue.credential.id(),
            })
            .await?;
        let InspectionRecords::Credentials { records, .. } = view.records else {
            anyhow::bail!("invalid credential metadata");
        };
        let record = records.first().context("gateway credential absent")?;
        keys.push(GatewayKeyView {
            id: key.issue.id.to_string(),
            label: key.issue.label,
            expires: record.scope.expires.0,
            revoked: record.revoked || record.generation != view.generation,
        });
    }
    keys.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(keys)
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
    let key =
        vault::read::<NamedKey>(&path(&local.directory, id)?)?.context("gateway key not found")?;
    let view = session
        .inspect(InspectionQuery::Credential {
            id: key.issue.credential.id(),
        })
        .await?;
    let InspectionRecords::Credentials { records, .. } = view.records else {
        anyhow::bail!("invalid credential metadata");
    };
    let record = records.first().context("gateway credential absent")?;
    if !record.revoked && record.generation == view.generation {
        session.revoke(record.id, record.revision).await?;
    }
    Ok(())
}
