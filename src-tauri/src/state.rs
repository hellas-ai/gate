use crate::{control::LocalNode, dto::*, history::History, identity::Identity, vault::Vault};
use anyhow::Context;
use hellas_rpc::pb::host::GetHostStatusRequest;
use hellas_sdk::node_control::host::HostHandle;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Mutex;

pub struct AppState {
    pub history: History,
    pub directory: PathBuf,
    pub vault: Vault,
    local: Mutex<Option<Arc<LocalNode>>>,
    host: Mutex<Option<HostHandle>>,
    operation: Mutex<()>,
    stopping: AtomicBool,
}
impl AppState {
    pub async fn open(directory: &Path) -> anyhow::Result<Self> {
        Self::with_vault(directory, Vault::native(directory)?).await
    }
    pub(crate) async fn with_vault(directory: &Path, vault: Vault) -> anyhow::Result<Self> {
        hellas_private::create_dir_all_durable(directory)?;
        hellas_private::restrict_directory(directory)?;
        let state = Self {
            history: History::open(&directory.join("history.sqlite3"))?,
            directory: directory.to_owned(),
            vault,
            local: Mutex::new(None),
            host: Mutex::new(None),
            operation: Mutex::new(()),
            stopping: AtomicBool::new(false),
        };
        if let Some(identity) = Identity::load(directory, &state.vault).await? {
            state.mount(identity).await?;
        }
        Ok(state)
    }
    async fn mount(&self, identity: Identity) -> anyhow::Result<()> {
        let (local, host) = LocalNode::open(&self.directory, identity, self.vault.clone()).await?;
        *self.host.lock().await = Some(host);
        *self.local.lock().await = Some(local);
        Ok(())
    }
    pub async fn initialize_software(&self, name: String) -> anyhow::Result<AppStatus> {
        let _lock = self.operation.lock().await;
        anyhow::ensure!(
            !self.stopping.load(Ordering::SeqCst),
            "Gate is shutting down"
        );
        anyhow::ensure!(
            self.local.lock().await.is_none(),
            "identity already initialized"
        );
        let identity = Identity::create(&self.directory, &self.vault, name).await?;
        self.mount(identity).await?;
        self.status().await
    }
    pub async fn local(&self) -> anyhow::Result<Arc<LocalNode>> {
        anyhow::ensure!(
            !self.stopping.load(Ordering::SeqCst),
            "Gate is shutting down"
        );
        self.local
            .lock()
            .await
            .clone()
            .context("create or select an owner passkey before using Gate")
    }
    pub async fn status(&self) -> anyhow::Result<AppStatus> {
        let stopped = || ServiceStatus {
            state: "stopped".into(),
            detail: String::new(),
        };
        let Some(local) = self.local.lock().await.clone() else {
            return Ok(AppStatus { version: env!("CARGO_PKG_VERSION").into(), identity: IdentityStatus { initialized: false, principal: String::new(), fingerprint: String::new(), node_key: String::new(), transport_id: String::new(), authority: String::new(), generation: 0, attestation: "unavailable".into(), detail: "Native passkey bridge unavailable. Explicit software identity is for development; user presence only.".into(), contact_card: String::new() }, provider: stopped(), gateway: stopped(), endpoint: String::new(), socket_path: self.directory.join("gate.sock").display().to_string() });
        };
        let status = local
            .client(false)
            .await?
            .get_host_status(GetHostStatusRequest {
                authorization: local.authorization(),
            })
            .await?;
        let service = |s: Option<hellas_rpc::pb::host::ServiceStatus>| {
            s.map(|s| ServiceStatus {
                state: match s.state {
                    1 => "starting",
                    2 => "running",
                    3 => "stopping",
                    4 => "failed",
                    _ => "stopped",
                }
                .into(),
                detail: s.detail,
            })
            .unwrap_or_else(stopped)
        };
        use base64::Engine;
        let identity = &local.identity;
        Ok(AppStatus { version: status.version, identity: IdentityStatus { initialized: true, principal: identity.public.owner.to_string(), fingerprint: identity.public.owner.fingerprint(), node_key: hex::encode(identity.node.node_key().public_key().bytes()), transport_id: identity.node.node_id().to_string(), authority: identity.public.authority.to_string(), generation: status.generation, attestation: "software".into(), detail: "Software P-256 passkey (UP only); execution is non-private unless independently attested.".into(), contact_card: base64::engine::general_purpose::STANDARD.encode(hellas_rpc::protocol::value::canonical_dag_cbor(&identity.public.card)?) }, provider: service(status.provider), gateway: service(status.gateway), endpoint: status.endpoint, socket_path: local.socket.display().to_string() })
    }
    pub async fn shutdown(&self) -> anyhow::Result<bool> {
        let _lock = self.operation.lock().await;
        if self.stopping.swap(true, Ordering::SeqCst) {
            return Ok(false);
        }
        if let Some(host) = self.host.lock().await.take() {
            host.shutdown().await?;
        }
        self.local.lock().await.take();
        Ok(true)
    }
}
