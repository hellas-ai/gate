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
    failure: Mutex<Option<String>>,
    plans: Mutex<std::collections::BTreeMap<String, crate::apply::Plan>>,
    pub runs: Arc<tokio::sync::Semaphore>,
    pub cancellations: Mutex<std::collections::BTreeMap<String, tokio::sync::watch::Sender<bool>>>,
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
            failure: Mutex::new(None),
            plans: Mutex::new(std::collections::BTreeMap::new()),
            runs: Arc::new(tokio::sync::Semaphore::new(64)),
            cancellations: Mutex::new(std::collections::BTreeMap::new()),
        };
        let opened: anyhow::Result<()> = async {
            if let Some(identity) = Identity::load(directory, &state.vault).await? {
                state.mount(identity).await?;
            }
            Ok(())
        }
        .await;
        if let Err(error) = opened {
            *state.failure.lock().await = Some(format!("{error:#}"));
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
    pub(crate) async fn begin_run(
        &self,
        request: &RunRequest,
    ) -> anyhow::Result<(
        Arc<LocalNode>,
        tokio::sync::OwnedSemaphorePermit,
        String,
        tokio::sync::watch::Receiver<bool>,
    )> {
        let _lock = self.operation.lock().await;
        let local = self.local().await?;
        let permit = self.runs.clone().try_acquire_owned()?;
        let id = self.history.begin(request)?;
        let (cancel, cancelled) = tokio::sync::watch::channel(false);
        self.cancellations.lock().await.insert(id.clone(), cancel);
        Ok((local, permit, id, cancelled))
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
            return Ok(AppStatus { version: env!("CARGO_PKG_VERSION").into(), identity: IdentityStatus { initialized: false, recovery_available:self.directory.join("installation.json").try_exists()?, principal: String::new(), fingerprint: String::new(), node_key: String::new(), transport_id: String::new(), authority: String::new(), generation: 0, attestation: "unavailable".into(), detail: self.failure.lock().await.clone().unwrap_or_else(||"Native passkey bridge unavailable. Explicit software identity is for development; user presence only.".into()), contact_card: String::new() }, provider: stopped(), gateway: stopped(), endpoint: String::new(), socket_path: self.directory.join("gate.sock").display().to_string(), listen_port: crate::settings::load(&self.directory)?.listen_port, bound_addresses: vec![], min_omit_response_blocks: hellas_sdk::kernel::MIN_OMIT_RESPONSE_BLOCKS });
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
        let attestation = match local.coordinator.provider().await? {
            Some(enrollment)
                if matches!(
                    enrollment.platform,
                    hellas_rpc::PlatformEnrollment::AppleAppAttest(_)
                ) =>
            {
                "apple-app-attest"
            }
            Some(_) => "software (non-private)",
            None => "unbound",
        };
        Ok(AppStatus { version: status.version, identity: IdentityStatus { initialized: true, recovery_available:false, principal: identity.public.owner.to_string(), fingerprint: identity.public.owner.fingerprint(), node_key: hex::encode(identity.node.node_key().public_key().bytes()), transport_id: identity.node.node_id().to_string(), authority: identity.public.authority.to_string(), generation: status.generation, attestation: attestation.into(), detail: "Software P-256 passkey (UP only); execution is non-private unless independently attested.".into(), contact_card: base64::engine::general_purpose::STANDARD.encode(hellas_rpc::protocol::value::canonical_dag_cbor(&identity.public.card)?) }, provider: service(status.provider), gateway: service(status.gateway), endpoint: status.endpoint, socket_path: local.socket.display().to_string(), listen_port: crate::settings::load(&self.directory)?.listen_port, bound_addresses: local.bound_addresses.get().cloned().unwrap_or_default(), min_omit_response_blocks: hellas_sdk::kernel::MIN_OMIT_RESPONSE_BLOCKS })
    }
    pub async fn shutdown(&self) -> anyhow::Result<bool> {
        let _lock = self.operation.lock().await;
        if self.stopping.swap(true, Ordering::SeqCst) {
            return Ok(false);
        }
        if let Some(host) = self.host.lock().await.take() {
            host.shutdown().await?;
        }
        if let Some(local) = self.local.lock().await.take() {
            local.clients.shutdown().await?;
        }
        self.plans.lock().await.clear();
        let _drained = self.runs.acquire_many(64).await?;
        Ok(true)
    }
}

impl AppState {
    pub async fn bind_provider(&self, software: bool) -> anyhow::Result<AppStatus> {
        let _lock = self.operation.lock().await;
        let local = self.local().await?;
        let identity = crate::provider_identity::ProviderIdentity::load_or_create(
            &self.directory,
            &local,
            software,
        )
        .await?;
        local
            .client(true)
            .await?
            .bind_provider(hellas_rpc::pb::host::BindProviderRequest {
                authorization: local.authorization(),
                enrollment: identity.enrollment.canonical_bytes(),
            })
            .await?;
        self.status().await
    }
    pub async fn configure_provider(
        &self,
        expected: u64,
        change: ProviderChange,
    ) -> anyhow::Result<ProviderView> {
        let _lock = self.operation.lock().await;
        let local = self.local().await?;
        let current = local.composition.view()?.revision;
        anyhow::ensure!(
            expected == current,
            "provider revision conflict: expected {expected}, current {current}"
        );
        let update = crate::provider_setup::update(expected, change);
        local
            .client(hellas_sdk::node_control::configuration::requires_passkey(
                &update,
            ))
            .await?
            .configure_provider(hellas_rpc::pb::host::ConfigureProviderRequest {
                authorization: local.authorization(),
                update: Some(update),
            })
            .await?;
        local.composition.view()
    }
    pub async fn provider_view(&self) -> anyhow::Result<ProviderView> {
        self.local().await?.composition.view()
    }
    pub async fn set_provider(&self, enabled: bool) -> anyhow::Result<AppStatus> {
        let _lock = self.operation.lock().await;
        let local = self.local().await?;
        local
            .client(false)
            .await?
            .set_provider_state(hellas_rpc::pb::host::SetProviderStateRequest {
                authorization: local.authorization(),
                enabled,
            })
            .await?;
        self.status().await
    }
    pub async fn configure_resource(
        &self,
        expected: u64,
        input: ResourceInput,
    ) -> anyhow::Result<ProviderView> {
        let _lock = self.operation.lock().await;
        let local = self.local().await?;
        let session = local.session(false).await?;
        crate::provider_setup::configure_resource(&local, &session, expected, input).await
    }
}

impl AppState {
    pub async fn contacts(&self) -> anyhow::Result<Vec<ContactView>> {
        self.local().await?;
        crate::contacts::contacts(&self.directory)
    }
    pub async fn add_contact(&self, input: ContactInput) -> anyhow::Result<ContactView> {
        let _lock = self.operation.lock().await;
        self.local().await?;
        crate::contacts::add(&self.directory, input)
    }
    pub async fn shares(&self) -> anyhow::Result<Vec<ShareView>> {
        self.local().await?;
        crate::contacts::shares(&self.directory)
    }
    pub async fn share(&self, input: ShareInput) -> anyhow::Result<ShareView> {
        let _lock = self.operation.lock().await;
        crate::contacts::share(&self.directory, self.local().await?.as_ref(), input).await
    }
    pub async fn export_access(&self, id: &str) -> anyhow::Result<String> {
        let _lock = self.operation.lock().await;
        crate::contacts::export(&self.directory, self.local().await?.as_ref(), id).await
    }
    pub async fn import_access(&self, bundle: &str) -> anyhow::Result<AccessView> {
        let _lock = self.operation.lock().await;
        crate::contacts::import(&self.directory, self.local().await?.as_ref(), bundle).await
    }
    pub async fn accesses(&self) -> anyhow::Result<Vec<AccessView>> {
        self.local().await?;
        crate::contacts::accesses(&self.directory)
    }
    pub async fn revoke_share(&self, id: &str) -> anyhow::Result<()> {
        let _lock = self.operation.lock().await;
        crate::contacts::revoke(&self.directory, self.local().await?.as_ref(), id).await
    }
    pub async fn pause_allowance(&self, id: &str, paused: bool) -> anyhow::Result<()> {
        let _lock = self.operation.lock().await;
        crate::contacts::pause(self.local().await?.as_ref(), id, paused).await
    }
}

impl AppState {
    pub async fn set_gateway(
        &self,
        enabled: bool,
        config: Option<GatewayConfig>,
    ) -> anyhow::Result<AppStatus> {
        let _lock = self.operation.lock().await;
        let local = self.local().await?;
        let session = local.session(false).await?;
        crate::gateway::configure(&local, &session, enabled, config).await?;
        self.status().await
    }
    pub async fn gateway_address(&self) -> anyhow::Result<String> {
        let local = self.local().await?;
        let access = local
            .client(false)
            .await?
            .get_gateway_access(hellas_rpc::pb::host::GetGatewayAccessRequest {
                authorization: local.authorization(),
            })
            .await?;
        Ok(access.address)
    }
    pub async fn reconcile_gateway_key(
        &self,
        id: &str,
    ) -> anyhow::Result<(GatewayKeyView, String)> {
        let _lock = self.operation.lock().await;
        crate::gateway::reconcile(self.local().await?.as_ref(), id).await
    }
    pub async fn gateway_keys(&self) -> anyhow::Result<Vec<GatewayKeyView>> {
        crate::gateway::list(self.local().await?.as_ref()).await
    }
    pub async fn issue_gateway_key(
        &self,
        input: GatewayKeyInput,
    ) -> anyhow::Result<(GatewayKeyView, String)> {
        let _lock = self.operation.lock().await;
        crate::gateway::issue(self.local().await?.as_ref(), input).await
    }
    pub async fn revoke_gateway_key(&self, id: &str) -> anyhow::Result<()> {
        let _lock = self.operation.lock().await;
        crate::gateway::revoke(self.local().await?.as_ref(), id).await
    }
    pub async fn save_paid_pool(&self, input: PaidPoolInput) -> anyhow::Result<()> {
        let _lock = self.operation.lock().await;
        self.local().await?;
        crate::paid::save_pool(&self.directory, input)
    }
}

impl AppState {
    pub async fn recover_software(&self) -> anyhow::Result<AppStatus> {
        let _lock = self.operation.lock().await;
        anyhow::ensure!(
            !self.stopping.load(Ordering::SeqCst),
            "Gate is shutting down"
        );
        anyhow::ensure!(
            self.local.lock().await.is_none(),
            "stop this node before offline owner recovery"
        );
        let identity = Identity::recover_software(&self.directory, &self.vault).await?;
        self.mount(identity).await?;
        *self.failure.lock().await = None;
        self.status().await
    }
}

impl AppState {
    pub async fn issue_admin_access(&self, input: AdminAccessInput) -> anyhow::Result<AdminAccess> {
        let _lock = self.operation.lock().await;
        crate::credentials::issue_admin(self.local().await?.as_ref(), input).await
    }
    pub async fn rotate_gateway_key(
        &self,
        old: &str,
        input: GatewayKeyInput,
    ) -> anyhow::Result<(GatewayKeyView, String)> {
        let _lock = self.operation.lock().await;
        crate::gateway::rotate(self.local().await?.as_ref(), old, input).await
    }
}

impl AppState {
    pub async fn preview_apply(&self, document: DesiredState) -> anyhow::Result<ApplyPreview> {
        let _lock = self.operation.lock().await;
        let local = self.local().await?;
        let plan = crate::apply::plan(&local, document).await?;
        let id = hex::encode(crate::identity::random::<16>()?);
        let preview = plan.preview(id.clone());
        let mut plans = self.plans.lock().await;
        anyhow::ensure!(plans.len() < 8, "discard or apply earlier previews first");
        plans.insert(id, plan);
        Ok(preview)
    }
    pub async fn apply_document(&self, id: &str) -> anyhow::Result<ApplyReport> {
        let _lock = self.operation.lock().await;
        let local = self.local().await?;
        let plan = self
            .plans
            .lock()
            .await
            .remove(id)
            .context("preview is missing or already consumed; preview the document again")?;
        crate::apply::execute(&local, plan).await
    }
    pub async fn quota(&self, id: &str) -> anyhow::Result<QuotaView> {
        let _lock = self.operation.lock().await;
        crate::contacts::quota(self.local().await?.as_ref(), id).await
    }
    pub async fn discard_apply(&self, id: &str) {
        self.plans.lock().await.remove(id);
    }
    pub async fn set_quota(&self, input: QuotaInput) -> anyhow::Result<()> {
        let _lock = self.operation.lock().await;
        let local = self.local().await?;
        let session = local.session(false).await?;
        crate::contacts::set_quota(&local, &session, input).await
    }
}

impl AppState {
    pub async fn paid_client_peer(&self, pool_id: &str) -> anyhow::Result<String> {
        let _lock = self.operation.lock().await;
        let local = self.local().await?;
        local.clients.paid_peer(&local, pool_id).await
    }
    pub async fn configure_paid_serve(
        &self,
        expected: u64,
        input: Option<PaidServeInput>,
    ) -> anyhow::Result<ProviderView> {
        let _lock = self.operation.lock().await;
        let local = self.local().await?;
        let session = local.session(false).await?;
        local
            .composition
            .configure_paid(&local, &session, expected, input)
            .await
    }
    pub async fn provision_paid(
        &self,
        input: ProvisionInput,
        commit: bool,
    ) -> anyhow::Result<ProvisionView> {
        let _lock = self.operation.lock().await;
        crate::paid::provision(self.local().await?.as_ref(), input, commit).await
    }
}

impl AppState {
    pub async fn save_listen_port(&self, port: Option<u16>) -> anyhow::Result<AppStatus> {
        let _lock = self.operation.lock().await;
        anyhow::ensure!(
            !self.stopping.load(Ordering::SeqCst),
            "Gate is shutting down"
        );
        let local = self.local.lock().await.clone();
        let current = if let Some(local) = local {
            local.session(false).await?.status().await?;
            local.bound_addresses.get().cloned().unwrap_or_default()
        } else {
            vec![]
        };
        crate::settings::save(&self.directory, port, &current)?;
        self.status().await
    }
    pub async fn retry_open(&self) -> anyhow::Result<AppStatus> {
        let _lock = self.operation.lock().await;
        anyhow::ensure!(
            !self.stopping.load(Ordering::SeqCst),
            "Gate is shutting down"
        );
        anyhow::ensure!(self.local.lock().await.is_none(), "Gate is already open");
        let identity = Identity::load(&self.directory, &self.vault)
            .await?
            .context("initialize an identity first")?;
        self.mount(identity).await?;
        *self.failure.lock().await = None;
        self.status().await
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[tokio::test(flavor = "multi_thread")]
    async fn missing_application_secret_requires_explicit_owner_recovery_and_rotates_generation() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let vault = Vault::fixture(secrets.path());
        let app = AppState::with_vault(directory.path(), vault.clone())
            .await
            .unwrap();
        let before = app.initialize_software("Alice".into()).await.unwrap();
        let (_, bearer) = app
            .issue_gateway_key(GatewayKeyInput {
                request_id: hex::encode([71; 16]),
                label: "old generation".into(),
                period: crate::period::Period::Week,
            })
            .await
            .unwrap();
        app.shutdown().await.unwrap();
        drop(app);
        vault.delete("application-credential").await.unwrap();
        let app = AppState::with_vault(directory.path(), vault.clone())
            .await
            .unwrap();
        let broken = app.status().await.unwrap();
        assert!(!broken.identity.initialized);
        assert!(broken.identity.recovery_available);
        assert!(
            app.initialize_software("Silent replacement".into())
                .await
                .is_err()
        );
        let recovered = app.recover_software().await.unwrap();
        assert_eq!(recovered.identity.generation, 2);
        assert_eq!(before.identity.principal, recovered.identity.principal);
        assert_eq!(before.identity.node_key, recovered.identity.node_key);
        assert!(
            app.gateway_keys()
                .await
                .unwrap()
                .iter()
                .all(|key| key.state == GatewayKeyState::Revoked)
        );
        let local = app.local().await.unwrap();
        let bytes = hex::decode(bearer).unwrap();
        let old = crate::identity::decode_credential(&bytes).unwrap();
        assert!(local.coordinator.authenticate(&old).await.is_err());
        app.shutdown().await.unwrap();
        drop(local);
        drop(app);
        let reopened = AppState::with_vault(directory.path(), vault).await.unwrap();
        assert_eq!(reopened.status().await.unwrap().identity.generation, 2);
        reopened.shutdown().await.unwrap();
    }
}
