//! Client-prepared issuance. Exact IDs survive lost replies; no secret is journaled.
use crate::{
    identity::{self, Identity},
    period::Period,
    vault::{self, Vault},
};
use anyhow::{Context, ensure};
use hellas_auth::authority::*;
use hellas_sdk::work::node::{NodeCoordinator, SessionKey};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedIssue {
    pub id: GrantId,
    pub credential: Credential,
    pub label: String,
    pub generation: u64,
}
impl PreparedIssue {
    pub async fn prepare(
        path: &Path,
        vault: &Vault,
        id: GrantId,
        scope: GrantSpec,
        holder: CredentialHolder,
        generation: u64,
        label: String,
    ) -> anyhow::Result<Self> {
        if let Some(held) = vault::read::<Self>(path)? {
            ensure!(
                held.id == id
                    && held.credential.scope() == &scope
                    && held.credential.holder() == holder,
                "prepared issuance conflicts with desired scope"
            );
            vault.required(&Self::key(held.credential.id())).await?;
            return Ok(held);
        }
        let (credential, secret) = Credential::prepare(scope, holder, generation)?;
        identity::save_credential(vault, &Self::key(secret.id()), &secret).await?;
        let held = Self {
            id,
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
            .authenticate_session(&identity.credential, SessionKey::Passkey(&identity.passkey))
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
            Err(hellas_sdk::work::work_store::grant::NodeStoreError::Authority(
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
            Err(hellas_sdk::work::work_store::grant::NodeStoreError::Authority(
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
                permissions: [hellas_sdk::work::operations::Operation::named(
                    hellas_sdk::work::node::GATEWAY_INVOKE,
                )?
                .permission(identity.public.authority, None)?]
                .into(),
                not_before: now,
                expires: Period::Unlimited
                    .expires(now)?
                    .min(identity.public.owner_scope.expires),
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
                GrantId(identity::random()?),
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
