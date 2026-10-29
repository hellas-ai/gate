//! Client-prepared issuance. Exact IDs survive lost replies; no secret is journaled.
use crate::{
    identity::{self, Identity},
    period::Period,
    vault::{self, Vault},
};
use anyhow::{Context, ensure};
use hellas_auth::authority::*;
use hellas_authority::node::{NodeCoordinator, SessionKey};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedIssue {
    pub id: GrantId,
    pub intent: hellas_auth::Digest,
    pub credential: Credential,
    pub label: String,
    pub generation: u64,
}
impl PreparedIssue {
    pub async fn prepare(
        path: &Path,
        vault: &Vault,
        request: (GrantId, hellas_auth::Digest),
        scope: GrantSpec,
        holder: CredentialHolder,
        generation: u64,
        label: String,
    ) -> anyhow::Result<Self> {
        let (id, intent) = request;
        // Intent and the original timestamped verifier share one durable publication.
        // An outer index may fail later; retries recover this exact scope and secret.
        if let Some(held) = vault::read::<Self>(path)? {
            ensure!(
                held.id == id
                    && held.intent == intent
                    && held.generation == generation
                    && held.credential.scope().authority == scope.authority
                    && held.credential.scope().subject == scope.subject
                    && held.credential.scope().permissions == scope.permissions
                    && held.credential.holder() == holder,
                "prepared issuance conflicts with desired intent or generation"
            );
            vault.required(&Self::key(held.credential.id())).await?;
            return Ok(held);
        }
        let (credential, secret) = Credential::prepare(scope, holder, generation)?;
        identity::save_credential(vault, &Self::key(secret.id()), &secret).await?;
        let held = Self {
            id,
            intent,
            credential,
            label,
            generation,
        };
        vault::write(path, &held)?;
        Ok(held)
    }
    pub fn key(id: CredentialId) -> String {
        format!("credential-{id}")
    }
    pub async fn secret(&self, vault: &Vault) -> anyhow::Result<CredentialSecret> {
        identity::decode_credential(&vault.required(&Self::key(self.credential.id())).await?)
    }
    pub async fn reconcile(
        &self,
        node: &NodeCoordinator,
        identity: &Identity,
        vault: &Vault,
    ) -> anyhow::Result<()> {
        let caller = node
            .authenticate_session(
                &identity.credential,
                SessionKey::Passkey(identity.passkey.as_ref()),
            )
            .await?;
        self.reconcile_as(node, &caller, identity.public.root, vault)
            .await
    }
    pub async fn reconcile_as(
        &self,
        node: &NodeCoordinator,
        caller: &AuthenticatedCaller,
        root: GrantId,
        vault: &Vault,
    ) -> anyhow::Result<()> {
        match node
            .inspect(caller, root, InspectionQuery::Grant { id: self.id })
            .await
        {
            Ok(view) => {
                let InspectionRecords::Grants { records, .. } = view.records else {
                    anyhow::bail!("invalid grant inspection");
                };
                let grant = records.first().context("issued grant is absent")?;
                ensure!(
                    !grant.revoked() && grant.spec() == self.credential.scope(),
                    "issued grant differs from prepared intent"
                );
            }
            Err(hellas_authority::work_store::grant::NodeStoreError::Authority(
                AuthorityError::Denied,
            )) => {
                node.grant(
                    caller,
                    GrantCommand::Issue {
                        authorization: root,
                        id: self.id,
                        spec: self.credential.scope().clone(),
                    },
                )
                .await
                .context("committing the prepared grant")?;
            }
            Err(e) => return Err(e.into()),
        }
        match node
            .inspect(
                caller,
                root,
                InspectionQuery::Credential {
                    id: self.credential.id(),
                },
            )
            .await
        {
            Ok(view) => {
                let InspectionRecords::Credentials { records, .. } = view.records else {
                    anyhow::bail!("invalid credential inspection");
                };
                let record = records.first().context("issued credential is absent")?;
                ensure!(
                    !record.revoked
                        && record.scope == *self.credential.scope()
                        && record.holder == self.credential.holder(),
                    "issued credential differs from prepared intent"
                );
            }
            Err(hellas_authority::work_store::grant::NodeStoreError::Authority(
                AuthorityError::Denied,
            )) => {
                node.issue_credential(caller, root, self.credential.clone())
                    .await
                    .context("committing the prepared credential")?;
            }
            Err(e) => return Err(e.into()),
        }
        // Proof of the persisted secret is required, not just matching metadata.
        let secret = self.secret(vault).await?;
        match self.credential.holder() {
            CredentialHolder::Bearer => {
                node.authenticate(&secret)
                    .await
                    .context("authenticating the prepared bearer")?;
            }
            CredentialHolder::Node(_) => {}
        }
        Ok(())
    }
}
pub async fn gateway_anchor(
    directory: &Path,
    node: &NodeCoordinator,
    identity: &Identity,
    vault: &Vault,
) -> anyhow::Result<hellas_gateway::GatewayAccess> {
    let path = directory.join("gateway-listener-credential.json");
    let held = match vault::read::<PreparedIssue>(&path)? {
        Some(held) => held,
        None => {
            let now = UnixMillis::now()?;
            let scope = GrantSpec {
                authority: identity.public.authority,
                subject: identity.public.owner,
                permissions: [hellas_authority::operations::Operation::named(
                    hellas_authority::node::GATEWAY_INVOKE,
                )?
                .permission(identity.public.authority, None)?]
                .into(),
                not_before: now,
                expires: Period::Unlimited
                    .expires_within(now, identity.public.owner_scope.expires)?,
                delegation_depth: 0,
                issuance: None,
            };
            let caller = node
                .authenticate_session(
                    &identity.credential,
                    SessionKey::Node(identity.node.node_key()),
                )
                .await?;
            let generation = node.status(&caller, identity.public.root).await?.generation;
            PreparedIssue::prepare(
                &path,
                vault,
                (
                    GrantId(identity::random()?),
                    hellas_auth::Digest::hash(b"hellas.gate.gateway-listener.v1"),
                ),
                scope,
                CredentialHolder::Bearer,
                generation,
                "Gate listener".into(),
            )
            .await?
        }
    };
    held.reconcile(node, identity, vault)
        .await
        .context("reconciling gateway listener authority")?;
    Ok(
        hellas_gateway::GatewayAccess::new(node.clone(), held.id, held.secret(vault).await?)
            .await?,
    )
}

impl PreparedIssue {
    pub async fn reconcile_session(
        &self,
        session: &crate::control::Session,
        vault: &Vault,
    ) -> anyhow::Result<()> {
        ensure!(
            self.generation == session.status().await?.generation,
            "credential invalidated by owner recovery; explicitly reissue it"
        );
        match session
            .inspect(InspectionQuery::Grant { id: self.id })
            .await
        {
            Ok(view) => {
                let InspectionRecords::Grants { records, .. } = view.records else {
                    anyhow::bail!("invalid grant inspection");
                };
                let grant = records.first().context("issued grant absent")?;
                ensure!(
                    !grant.revoked() && grant.spec() == self.credential.scope(),
                    "issued grant differs from prepared intent"
                );
            }
            Err(e)
                if e.downcast_ref::<hellas_wire::WireStatus>()
                    .is_some_and(|s| s.code() == hellas_wire::WireCode::PermissionDenied) =>
            {
                session
                    .grant(GrantCommand::Issue {
                        authorization: GrantId(session.authorization.as_slice().try_into()?),
                        id: self.id,
                        spec: self.credential.scope().clone(),
                    })
                    .await?;
            }
            Err(e) => return Err(e),
        }
        self.reconcile_credential(session).await?;
        self.secret(vault).await?;
        Ok(())
    }
    pub async fn reconcile_credential(
        &self,
        session: &crate::control::Session,
    ) -> anyhow::Result<()> {
        match session
            .inspect(InspectionQuery::Credential {
                id: self.credential.id(),
            })
            .await
        {
            Ok(view) => {
                let InspectionRecords::Credentials { records, .. } = view.records else {
                    anyhow::bail!("invalid credential inspection");
                };
                let record = records.first().context("issued credential absent")?;
                ensure!(
                    !record.revoked && record.generation == view.generation,
                    "credential revoked by owner recovery; explicitly reissue it"
                );
                ensure!(
                    record.scope == *self.credential.scope()
                        && record.holder == self.credential.holder(),
                    "issued credential differs from prepared intent"
                );
            }
            Err(e)
                if e.downcast_ref::<hellas_wire::WireStatus>()
                    .is_some_and(|s| s.code() == hellas_wire::WireCode::PermissionDenied) =>
            {
                session.issue(&self.credential).await?;
            }
            Err(e) => return Err(e),
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdminPrepared {
    intent: hellas_auth::Digest,
    issue: PreparedIssue,
}
pub async fn issue_admin(
    local: &crate::control::LocalNode,
    mut input: crate::dto::AdminAccessInput,
) -> anyhow::Result<crate::dto::AdminAccess> {
    input.request_id = input.request_id.parse::<GrantId>()?.to_string();
    ensure!(
        !input.label.trim().is_empty() && input.label.len() <= 64,
        "name the CLI device"
    );
    let holder = NodeKey::from_public_key(hellas_auth::PublicKey::Secp256k1(crate::trust::fixed(
        &input.holder,
    )?))?;
    let path = local
        .directory
        .join("admin-access")
        .join(format!("{}.json", input.request_id));
    let intent = hellas_auth::Digest::hash(&serde_json::to_vec(&input)?);
    let session = local.session(true).await?;
    let held = match vault::read::<AdminPrepared>(&path)? {
        Some(held) => {
            ensure!(
                held.intent == intent,
                "prepared CLI access ID conflicts with this intent"
            );
            held
        }
        None => {
            let now = UnixMillis::now()?;
            let mut scope = local.identity.public.owner_scope.clone();
            let status = session.status().await?;
            let bound = local.coordinator.provider().await?.is_some();
            let available = |name: &str| {
                use hellas_authority::operations::{Mount, OPERATIONS};
                OPERATIONS
                    .iter()
                    .find(|operation| operation.name == name)
                    .is_none_or(|operation| match operation.mount {
                        Mount::Host => true,
                        Mount::Work => bound,
                        Mount::Machine => {
                            bound
                                && status
                                    .machine
                                    .as_ref()
                                    .is_some_and(|machine| machine.max_in_flight > 0)
                        }
                    })
            };
            // Select only currently mounted service capabilities; the SDK validates
            // this scope again before issuing any verifier.
            scope
                .permissions
                .retain(|permission| available(permission.operation.as_str()));
            if let Some(ceiling) = &mut scope.issuance {
                ceiling
                    .operations
                    .retain(|operation| available(operation.as_str()));
            }
            scope.not_before = now;
            scope.expires = input.period.expires_within(now, scope.expires)?;
            let generation = session.status().await?.generation;
            let issue = PreparedIssue::prepare(
                &local
                    .directory
                    .join("prepared-admin-access")
                    .join(format!("{}.json", input.request_id)),
                &local.vault,
                (local.identity.public.root, intent),
                scope,
                CredentialHolder::Node(holder),
                generation,
                input.label,
            )
            .await?;
            let held = AdminPrepared { intent, issue };
            vault::write(&path, &held)?;
            held
        }
    };
    held.issue.reconcile_credential(&session).await?;
    let credential = held.issue.secret(&local.vault).await?;
    use base64::Engine;
    Ok(crate::dto::AdminAccess {
        authority: local.identity.public.authority.to_string(),
        authorization: local.identity.public.root.to_string(),
        holder: hex::encode(holder.as_bytes()),
        expires: held.issue.credential.scope().expires.0,
        socket_path: local.socket.display().to_string(),
        certificate_base64: base64::engine::general_purpose::STANDARD
            .encode(&local.identity.public.certificate),
        server_name: "localhost".into(),
        credential_hex: hex::encode(credential_bytes_for_handoff(&credential)),
    })
}
fn credential_bytes_for_handoff(credential: &CredentialSecret) -> zeroize::Zeroizing<Vec<u8>> {
    let mut bytes = zeroize::Zeroizing::new(credential.id().0.to_vec());
    bytes.extend_from_slice(credential.expose_secret());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dto::*, state::AppState};
    use base64::{Engine, engine::general_purpose::STANDARD};
    #[tokio::test(flavor = "multi_thread")]
    async fn cli_handoff_requires_device_key_and_fresh_passkey_for_sensitive_calls() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let app = AppState::with_vault(directory.path(), Vault::fixture(secrets.path()))
            .await
            .unwrap();
        app.initialize_software("Owner".into()).await.unwrap();
        let cli = hellas_sdk::NodeIdentity::generate();
        let input = AdminAccessInput {
            request_id: hex::encode([117; 16]),
            label: "CLI device".into(),
            holder: hex::encode(cli.node_key().public_key().bytes()),
            period: Period::Week,
        };
        let handoff = app.issue_admin_access(input.clone()).await.unwrap();
        let again = app.issue_admin_access(input).await.unwrap();
        assert_eq!(handoff.credential_hex, again.credential_hex);
        let credential =
            crate::identity::decode_credential(&hex::decode(&handoff.credential_hex).unwrap())
                .unwrap();
        let local = app.local().await.unwrap();
        let (tls, server) = hellas_sdk::node_control::profile::local_trust(
            rustls::pki_types::CertificateDer::from(
                STANDARD.decode(handoff.certificate_base64).unwrap(),
            ),
            handoff.server_name,
        )
        .unwrap();
        let (client, _) = hellas_sdk::node_control::connect_local(
            &local.socket,
            tls.clone(),
            server.clone(),
            local.identity.public.authority,
            &credential,
            SessionKey::Node(cli.node_key()),
        )
        .await
        .unwrap();
        let status = client
            .get_host_status(hellas_rpc::pb::host::GetHostStatusRequest {
                authorization: local.authorization(),
            })
            .await
            .unwrap();
        assert_eq!(status.generation, 1);
        assert!(
            hellas_sdk::node_control::connect_local(
                &local.socket,
                tls.clone(),
                server.clone(),
                local.identity.public.authority,
                &credential,
                SessionKey::Node(local.identity.node.node_key())
            )
            .await
            .is_err()
        );
        let change = || hellas_rpc::pb::host::ConfigureProviderRequest {
            authorization: local.authorization(),
            update: Some(crate::provider_setup::update(
                0,
                ProviderChange::Endpoint {
                    service: "cli".into(),
                    method: "get".into(),
                    hosts: vec!["example.com".into()],
                    allow_private_addresses: false,
                },
            )),
        };
        assert_eq!(
            client
                .configure_provider(change())
                .await
                .unwrap_err()
                .code(),
            hellas_wire::WireCode::PermissionDenied
        );
        let (approved, _) = hellas_sdk::node_control::connect_local(
            &local.socket,
            tls,
            server,
            local.identity.public.authority,
            &credential,
            SessionKey::Passkey(local.identity.passkey.as_ref()),
        )
        .await
        .unwrap();
        approved.configure_provider(change()).await.unwrap();
        assert_eq!(app.provider_view().await.unwrap().revision, 1);
        app.shutdown().await.unwrap();
    }
}
