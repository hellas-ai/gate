//! Gate is a client of the same authenticated local socket/pipe as the CLI.
use crate::{identity::Identity, vault::Vault};
use anyhow::ensure;
use hellas_auth::authority::*;
use hellas_rpc::{pb::host::*, services::host_control::HostControlClientImpl};
use hellas_sdk::{
    node_control::{
        self,
        host::{HostHandle, HostOptions},
    },
    work::node::{NodeCoordinator, SessionKey},
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub type Client = HostControlClientImpl<hellas_wire::mux::MuxTransport>;
pub struct LocalNode {
    pub identity: Arc<Identity>,
    pub coordinator: NodeCoordinator,
    pub socket: PathBuf,
    pub vault: Vault,
    tls: Arc<rustls::ClientConfig>,
}
impl LocalNode {
    pub async fn open(
        directory: &Path,
        identity: Identity,
        vault: Vault,
    ) -> anyhow::Result<(Arc<Self>, HostHandle)> {
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
        let local = Arc::new(Self {
            identity: Arc::new(identity),
            coordinator: coordinator.clone(),
            socket: socket.clone(),
            vault,
            tls,
        });
        let host = HostHandle::start(HostOptions {
            transport: None,
            start_provider: false,
            control: Some((
                coordinator,
                hellas_sdk::ProviderControl {
                    socket,
                    tls: server,
                    remote: Default::default(),
                },
            )),
            provider: None,
            gateway: None,
        })
        .await?;
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
        Ok((local, host))
    }
    pub fn authorization(&self) -> Vec<u8> {
        self.identity.public.root.0.to_vec()
    }
    pub async fn client(&self, step_up: bool) -> anyhow::Result<Client> {
        let key = if step_up {
            SessionKey::Passkey(&self.identity.passkey)
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
            assert!(!metadata.windows(secret.len()).any(|w| w == secret.as_slice()));
        }
    }
}
