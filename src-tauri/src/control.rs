//! Gate is a client of the same authenticated local socket/pipe as the CLI.
use crate::{identity::Identity, vault::Vault};
use anyhow::ensure;
use hellas_auth::authority::*;
use hellas_authority::node::{NodeCoordinator, SessionKey};
use hellas_rpc::{pb::host::*, services::host_control::HostControlClientImpl};
use hellas_sdk::daemon::Core;
use hellas_sdk::node_control;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub fn home(directory: &Path) -> hellas_private::StoreLocation {
    hellas_private::StoreLocation::from_home(directory, hellas_private::Durability::Power)
}

pub type Client = HostControlClientImpl<hellas_wire::mux::MuxTransport>;
pub struct LocalNode {
    pub identity: Arc<Identity>,
    pub coordinator: NodeCoordinator,
    pub socket: PathBuf,
    pub vault: Vault,
    pub directory: PathBuf,
    pub clients: crate::clients::Clients,
    pub content: hellas_store::content::DiskStore,
    tls: Arc<rustls::ClientConfig>,
    core: Arc<Core>,
}
impl LocalNode {
    pub async fn open(
        directory: &Path,
        identity: Identity,
        vault: Vault,
    ) -> anyhow::Result<(Arc<Self>, hellas_wire::local::LocalControlServer)> {
        let store = identity.open_store(directory).await?;
        let coordinator =
            NodeCoordinator::new(store, Arc::new(|| UnixMillis::now().unwrap_or_default()));
        let certificate =
            rustls::pki_types::CertificateDer::from(identity.public.certificate.clone());
        let key = vault.required("control-tls-key").await?;
        let server = node_control::profile::server_tls(
            certificate.clone(),
            rustls::pki_types::PrivatePkcs8KeyDer::from(key.to_vec()),
        )?;
        let (tls, _) = node_control::profile::local_trust(certificate, "localhost".into())?;
        let socket = directory.join("gate.sock");
        let settlement_key = hellas_sdk::kernel::Secp256k1Signer::from_secret_scalar(
            identity.node.node_secret_bytes(),
        )
        .map_err(|_| anyhow::anyhow!("invalid provider settlement key"))?;
        let content = hellas_store::content::DiskStore::open(
            crate::control::home(directory).child("content"),
            vec![],
            hellas_store::content::IndexMode::Resume,
            None,
            hellas_store::content::Reservations::ram(
                hellas_channel::content::DEFAULT_MAX_STAGING_BYTES,
            ),
            false,
        )
        .await?;
        let opened = async {
            let chain = crate::paid::config(&crate::dto::ChainInput::Devnet, directory)?.chain;
            let core = Core::restore(hellas_sdk::daemon::Options {
                discovery: !cfg!(test),
                endpoint: None,
                remote_control: Default::default(),
                prover: Some(Arc::new(crate::provider_identity::ListenerProver {
                    vault: vault.clone(),
                    node: coordinator.clone(),
                })),
                home: home(directory),
                signer: Arc::new(identity.node.node_key().clone()),
                transport: identity.node.transport_key(),
                settlement: settlement_key,
                owner: Some(identity.passkey.clone()),
                chain,
                node: coordinator.clone(),
                content: content.clone(),
            })
            .await
            .map_err(anyhow::Error::from_boxed)?;
            let local = Arc::new(Self {
                identity: Arc::new(identity),
                coordinator: coordinator.clone(),
                socket: socket.clone(),
                directory: directory.to_owned(),
                clients: crate::clients::Clients::new(directory),
                content: content.clone(),
                vault,
                tls,
                core: core.clone(),
            });
            let control = match core
                .control()
                .map_err(anyhow::Error::from_boxed)?
                .bind_local(socket, server)
            {
                Ok(control) => control,
                Err(error) => {
                    let _ = core.shutdown().await;
                    return Err(error.into());
                }
            };
            let authenticated: anyhow::Result<()> = async {
                // A committed initialization must authenticate using precisely its prepared ID.
                let status = local
                    .client(false)
                    .await?
                    .get_host_status(GetHostStatusRequest {
                        authorization: local.authorization(),
                    })
                    .await?;
                ensure!(
                    status.authority == local.identity.public.authority.0.as_bytes(),
                    "local authority mismatch"
                );
                Ok(())
            }
            .await;
            if let Err(error) = authenticated {
                let _ = control.shutdown().await;
                let _ = core.shutdown().await;
                return Err(error);
            }
            Ok((local, control))
        }
        .await;
        if opened.is_err() {
            content.close().await;
        }
        opened
    }
    pub fn core(&self) -> &Arc<Core> {
        &self.core
    }
    pub fn authorization(&self) -> Vec<u8> {
        self.identity.public.root.0.to_vec()
    }
    pub async fn client(&self, step_up: bool) -> anyhow::Result<Client> {
        let key = if step_up {
            SessionKey::Passkey(self.identity.passkey.as_ref())
        } else {
            SessionKey::Node(self.identity.node.node_key())
        };
        Ok(node_control::connect_local(
            &self.socket,
            self.tls.clone(),
            "localhost".try_into()?,
            self.identity.public.authority,
            &self.identity.credential,
            key,
        )
        .await?
        .0)
    }
}

pub async fn work(
    client: &Client,
    authorization: Vec<u8>,
    command: hellas_authority::model::grant::admin::WorkControlCommand,
) -> anyhow::Result<hellas_authority::model::grant::admin::WorkControlReply> {
    use hellas_rpc::protocol::value::{canonical_dag_cbor, decode_dag_cbor};
    let reply = client
        .work_control(WorkControlRequest {
            authorization,
            command: canonical_dag_cbor(&command)?,
        })
        .await?;
    Ok(decode_dag_cbor(&reply.reply)?)
}

/// One authenticated SDK session reused for every operation in a document apply.
pub struct Session {
    pub client: Client,
    pub authorization: Vec<u8>,
    pub(crate) progress: std::sync::Mutex<crate::mutations::Progress>,
}
impl LocalNode {
    pub async fn session(&self, step_up: bool) -> anyhow::Result<Session> {
        Ok(Session::new(
            self.client(step_up).await?,
            self.authorization(),
        ))
    }
}
impl Session {
    pub(crate) fn new(client: Client, authorization: Vec<u8>) -> Self {
        Self {
            client,
            authorization,
            progress: Default::default(),
        }
    }
    pub async fn view(&self) -> anyhow::Result<hellas_sdk::daemon::View> {
        use futures::StreamExt;
        let mut updates = self
            .client
            .watch(WatchRequest {
                authorization: self.authorization.clone(),
            })
            .await?;
        let first = updates
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("daemon Watch ended before its snapshot"))??;
        let Some(watch_response::Update::Snapshot(snapshot)) = first.update else {
            anyhow::bail!("daemon Watch did not start with a snapshot");
        };
        Ok(serde_json::from_slice(&snapshot)?)
    }
    pub async fn configure(
        &self,
        expected: u64,
        document: hellas_sdk::daemon_config::Document,
    ) -> anyhow::Result<u64> {
        let response = self
            .track(
                format!("Configure daemon at revision {expected}"),
                true,
                self.client.configure(ConfigureRequest {
                    authorization: self.authorization.clone(),
                    expected_revision: expected,
                    document: serde_json::to_vec(&document)?,
                }),
            )
            .await?;
        Ok(response.revision)
    }
    pub async fn edit(
        &self,
        expected: Option<u64>,
        edit: impl FnOnce(&mut hellas_sdk::daemon_config::Document) -> anyhow::Result<()>,
    ) -> anyhow::Result<hellas_sdk::daemon::View> {
        let mut view = self.view().await?;
        if let Some(expected) = expected {
            check_revision(&view, expected)?;
        }
        edit(&mut view.document)?;
        crate::provider_setup::sync_listener(&mut view.document);
        self.configure(view.revision.0, view.document).await?;
        self.view().await
    }
    pub async fn inspect(&self, query: InspectionQuery) -> anyhow::Result<AuthorityInspection> {
        use hellas_rpc::protocol::value::{canonical_dag_cbor, decode_dag_cbor};
        let reply = self
            .client
            .inspect_authority(InspectAuthorityRequest {
                authorization: self.authorization.clone(),
                query: canonical_dag_cbor(&query)?,
            })
            .await?;
        Ok(decode_dag_cbor(&reply.reply)?)
    }
    pub async fn grant(&self, command: GrantCommand) -> anyhow::Result<()> {
        self.track(
            format!("Auth grant {command:?}"),
            true,
            self.client.grant_control(GrantControlRequest {
                command: hellas_rpc::protocol::value::canonical_dag_cbor(&command)?,
            }),
        )
        .await?;
        Ok(())
    }
    pub async fn issue(&self, credential: &Credential) -> anyhow::Result<()> {
        self.track(
            format!("Issue credential {}", credential.id()),
            true,
            self.client.issue_credential(IssueCredentialRequest {
                authorization: self.authorization.clone(),
                credential: hellas_rpc::protocol::value::canonical_dag_cbor(credential)?,
            }),
        )
        .await?;
        Ok(())
    }
    pub async fn revoke(&self, id: CredentialId, expected: Revision) -> anyhow::Result<()> {
        self.track(
            format!("Revoke credential {id} at revision {}", expected.0),
            true,
            self.client.revoke_credential(RevokeCredentialRequest {
                authorization: self.authorization.clone(),
                credential_id: id.0.to_vec(),
                expected_revision: expected.0,
            }),
        )
        .await?;
        Ok(())
    }
    pub async fn status(&self) -> anyhow::Result<HostStatus> {
        Ok(self
            .client
            .get_host_status(GetHostStatusRequest {
                authorization: self.authorization.clone(),
            })
            .await?)
    }
}

pub(crate) fn check_revision(view: &hellas_sdk::daemon::View, expected: u64) -> anyhow::Result<()> {
    anyhow::ensure!(view.revision.0 == expected, "provider revision conflict");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    #[tokio::test]
    async fn real_local_control_refuses_wrong_secret_key_and_pin_and_survives_restart() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let vault = Vault::fixture(secrets.path());
        let app = AppState::with_vault(directory.path(), vault.clone())
            .await
            .unwrap();
        let first = app.initialize_software("Alice".into()).await.unwrap();
        let node = app.local().await.unwrap();
        let secret = CredentialSecret::from_parts(node.identity.credential.id(), [77; 32]);
        assert!(
            node_control::connect_local(
                &node.socket,
                node.tls.clone(),
                "localhost".try_into().unwrap(),
                node.identity.public.authority,
                &secret,
                SessionKey::Node(node.identity.node.node_key())
            )
            .await
            .is_err()
        );
        let foreign = hellas_sdk::NodeIdentity::generate();
        assert!(
            node_control::connect_local(
                &node.socket,
                node.tls.clone(),
                "localhost".try_into().unwrap(),
                node.identity.public.authority,
                &node.identity.credential,
                SessionKey::Node(foreign.node_key())
            )
            .await
            .is_err()
        );
        let wrong_cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let (wrong_pin, name) =
            node_control::profile::local_trust(wrong_cert.cert.der().clone(), "localhost".into())
                .unwrap();
        assert!(
            node_control::connect_local(
                &node.socket,
                wrong_pin,
                name,
                node.identity.public.authority,
                &node.identity.credential,
                SessionKey::Node(node.identity.node.node_key())
            )
            .await
            .is_err()
        );
        assert!(app.shutdown().await.unwrap());
        assert!(!node.socket.exists());
        drop(node);
        drop(app);
        let app = AppState::with_vault(directory.path(), vault.clone())
            .await
            .unwrap();
        let second = app.status().await.unwrap();
        assert_eq!(first.identity.principal, second.identity.principal);
        assert_eq!(first.identity.node_key, second.identity.node_key);
        assert_eq!(first.identity.transport_id, second.identity.transport_id);
        assert_eq!(second.identity.generation, 1);
        assert!(app.local().await.unwrap().client(true).await.is_ok());
        app.shutdown().await.unwrap();
        let metadata = std::fs::read(directory.path().join("installation.json")).unwrap();
        for key in [
            "node-keys",
            "owner-software-passkey",
            "application-credential",
            "control-tls-key",
        ] {
            let secret = vault.required(key).await.unwrap();
            assert!(
                !metadata
                    .windows(secret.len())
                    .any(|w| w == secret.as_slice())
            );
        }
    }
}
