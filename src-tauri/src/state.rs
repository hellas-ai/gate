use crate::{control::LocalNode, dto::*, history::History, identity::Identity, vault::Vault};
use anyhow::Context;
use hellas_rpc::pb::host::GetHostStatusRequest;
use hellas_wire::local::LocalControlServer;
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
    control: Mutex<Option<LocalControlServer>>,
    operation: Mutex<()>,
    stopping: AtomicBool,
    shutdown_lock: Mutex<()>,
    opened: AtomicBool,
    failure: Mutex<Option<String>>,
    journal_failure: AtomicBool,
    plans: Mutex<std::collections::BTreeMap<String, crate::apply::Plan>>,
    pub runs: Arc<tokio::sync::Semaphore>,
    pub cancellations: Mutex<std::collections::BTreeMap<String, tokio::sync::watch::Sender<bool>>>,
}
impl AppState {
    /// Construct state without prompting: Tauri setup runs on the UI thread.
    pub fn open(directory: &Path) -> anyhow::Result<Self> {
        Self::prepare(directory, Vault::native(directory)?)
    }
    #[cfg(test)]
    pub(crate) async fn with_vault(directory: &Path, vault: Vault) -> anyhow::Result<Self> {
        let state = Self::prepare(directory, vault)?;
        state.open_existing().await?;
        Ok(state)
    }
    pub(crate) fn prepare(directory: &Path, vault: Vault) -> anyhow::Result<Self> {
        hellas_private::create_dir_all_durable(directory)?;
        hellas_private::restrict_directory(directory)?;
        let state = Self {
            history: History::open(&directory.join("history.sqlite3"))?,
            directory: directory.to_owned(),
            vault,
            local: Mutex::new(None),
            control: Mutex::new(None),
            operation: Mutex::new(()),
            stopping: AtomicBool::new(false),
            shutdown_lock: Mutex::new(()),
            opened: AtomicBool::new(false),
            failure: Mutex::new(None),
            journal_failure: AtomicBool::new(false),
            plans: Mutex::new(std::collections::BTreeMap::new()),
            runs: Arc::new(tokio::sync::Semaphore::new(64)),
            cancellations: Mutex::new(std::collections::BTreeMap::new()),
        };
        Ok(state)
    }
    /// First status request runs after the event loop starts, so native prompts
    /// can dispatch to the main queue. Failure is shown until an explicit retry.
    pub async fn open_existing(&self) -> anyhow::Result<()> {
        let _lock = self.operation.lock().await;
        anyhow::ensure!(
            !self.stopping.load(Ordering::SeqCst),
            "Gate is shutting down"
        );
        if self.opened.swap(true, Ordering::SeqCst) || self.local.lock().await.is_some() {
            return Ok(());
        }
        let opened: anyhow::Result<()> = async {
            if let Some(identity) = Identity::load(&self.directory, &self.vault).await? {
                self.mount(identity).await?;
            }
            Ok(())
        }
        .await;
        if let Err(error) = opened {
            self.journal_failure.store(
                error
                    .downcast_ref::<crate::identity::ExistingJournalError>()
                    .is_some(),
                Ordering::SeqCst,
            );
            *self.failure.lock().await = Some(format!("{error:#}"));
        }
        Ok(())
    }
    pub(crate) async fn mount(&self, identity: Identity) -> anyhow::Result<()> {
        let (local, control) =
            LocalNode::open(&self.directory, identity, self.vault.clone()).await?;
        *self.control.lock().await = Some(control);
        *self.local.lock().await = Some(local);
        Ok(())
    }
    pub async fn initialize_software(&self, name: String) -> anyhow::Result<AppStatus> {
        let _lock = self
            .operation
            .try_lock()
            .context("another Gate operation is already in progress")?;
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
    pub async fn initialize_native(&self, name: String) -> anyhow::Result<AppStatus> {
        let _lock = self
            .operation
            .try_lock()
            .context("another Gate operation is already in progress")?;
        anyhow::ensure!(
            !self.stopping.load(Ordering::SeqCst),
            "Gate is shutting down"
        );
        anyhow::ensure!(
            self.local.lock().await.is_none(),
            "identity already initialized"
        );
        let identity = Identity::create_native(&self.directory, &self.vault, name).await?;
        self.mount(identity).await?;
        self.status().await
    }
    pub async fn export_contact_card(&self) -> anyhow::Result<AppStatus> {
        let _lock = self
            .operation
            .try_lock()
            .map_err(|_| anyhow::anyhow!("another owner operation is in progress"))?;
        anyhow::ensure!(
            !self.stopping.load(Ordering::SeqCst),
            "Gate is shutting down"
        );
        let local = self.local().await?;
        local.identity.export_contact_card(&self.directory).await?;
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
            return Ok(AppStatus { version: env!("CARGO_PKG_VERSION").into(), identity: IdentityStatus { initialized: false, journal_error: self.journal_failure.load(Ordering::SeqCst), recovery_available:self.directory.join("installation.json").try_exists()? && !self.journal_failure.load(Ordering::SeqCst), principal: String::new(), fingerprint: String::new(), node_key: String::new(), transport_id: String::new(), provider_address: String::new(), authority: String::new(), generation: 0, attestation: "unavailable".into(), detail: self.failure.lock().await.clone().unwrap_or_else(||"Create your identity with a system passkey on macOS. Software identities are available explicitly for development.".into()), contact_card: String::new() }, provider: stopped(), gateway: stopped(), endpoint: String::new(), socket_path: self.directory.join("gate.sock").display().to_string(), listen_port: None, bound_addresses: vec![] });
        };
        let status = local
            .client(false)
            .await?
            .get_host_status(GetHostStatusRequest {
                authorization: local.authorization(),
            })
            .await?;
        let view = local.session(false).await?.view().await?;
        let service = |id: &str, enabled| match view.listener_errors.get(id) {
            Some(error) => ServiceStatus {
                state: "error".into(),
                detail: error.clone(),
            },
            None => ServiceStatus {
                state: if enabled { "running" } else { "stopped" }.into(),
                detail: String::new(),
            },
        };
        use base64::Engine;
        let identity = &local.identity;
        let enrollment = local.coordinator.provider().await?;
        let provider_address = enrollment
            .as_ref()
            .map(hellas_sdk::client::ProviderAddress::of)
            .transpose()?
            .map(|address| address.to_string())
            .unwrap_or_default();
        let attestation = match enrollment {
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
        Ok(AppStatus {
            version: status.version,
            identity: IdentityStatus {
                initialized: true,
                journal_error: false,
                recovery_available: false,
                principal: identity.public.owner.to_string(),
                fingerprint: identity.public.owner.fingerprint(),
                node_key: hex::encode(identity.node.node_key().public_key().bytes()),
                transport_id: identity.node.node_id().to_string(),
                provider_address,
                authority: identity.public.authority.to_string(),
                generation: status.generation,
                attestation: attestation.into(),
                detail: if identity.public.native_passkey.is_some() {
                    "System passkey; owner approvals require user verification."
                } else {
                    "Development software passkey; user presence only."
                }
                .into(),
                contact_card: identity
                    .contact_card(&self.directory)?
                    .map(|card| {
                        hellas_rpc::protocol::value::canonical_dag_cbor(&card)
                            .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes))
                    })
                    .transpose()?
                    .unwrap_or_default(),
            },
            provider: service("work", !view.iroh_addresses.is_empty()),
            gateway: service("gateway", !view.http_addresses.is_empty()),
            endpoint: view
                .http_addresses
                .values()
                .next()
                .map(|address| format!("http://{address}"))
                .unwrap_or_default(),
            socket_path: local.socket.display().to_string(),
            listen_port: view
                .document
                .listeners
                .get("work")
                .and_then(|listener| match listener {
                    hellas_sdk::daemon_config::Listener::Iroh { port, .. } => Some(*port),
                    _ => None,
                }),
            bound_addresses: view
                .iroh_addresses
                .values()
                .flat_map(|address| address.ip_addrs())
                .map(|address| address.to_string())
                .collect(),
        })
    }
    pub async fn shutdown(&self) -> anyhow::Result<bool> {
        // Concurrent exit/restart callers must wait for the first drain.
        let _shutdown = self.shutdown_lock.lock().await;
        if self.stopping.swap(true, Ordering::SeqCst) {
            return Ok(false);
        }
        crate::passkey::shutdown();
        let _lock = self.operation.lock().await;
        if let Some(control) = self.control.lock().await.take() {
            control.shutdown().await?;
        }
        if let Some(local) = self.local.lock().await.take() {
            let core = local
                .core()
                .shutdown()
                .await
                .map_err(anyhow::Error::from_boxed);
            local.coordinator.close().await;
            core?;
            local.content.close().await;
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
        let session = local.session(true).await?;
        crate::provider_setup::configure(&session, expected, change).await
    }
    pub async fn provider_view(&self) -> anyhow::Result<ProviderView> {
        crate::provider_setup::view(self.local().await?.as_ref()).await
    }
    pub async fn set_provider(&self, enabled: bool) -> anyhow::Result<AppStatus> {
        let _lock = self.operation.lock().await;
        let local = self.local().await?;
        local.session(true).await?.provider_state(enabled).await?;
        self.status().await
    }
    pub async fn configure_resource(
        &self,
        expected: u64,
        input: ResourceInput,
    ) -> anyhow::Result<ProviderView> {
        let _lock = self.operation.lock().await;
        let local = self.local().await?;
        let session = local.session(true).await?;
        crate::provider_setup::configure_resource(&session, expected, input).await
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
        let _lock = self
            .operation
            .try_lock()
            .map_err(|_| anyhow::anyhow!("another owner operation is already in progress"))?;
        anyhow::ensure!(
            !self.stopping.load(Ordering::SeqCst),
            "Gate is shutting down"
        );
        let local = self.local().await?;
        let session = local.session(true).await?;
        crate::gateway::configure(&local, &session, enabled, config).await?;
        self.status().await
    }
    pub async fn gateway_address(&self) -> anyhow::Result<String> {
        let local = self.local().await?;
        let view = local.session(false).await?.view().await?;
        let address = view
            .http_addresses
            .values()
            .next()
            .context("gateway is stopped")?;
        Ok(format!("http://{address}"))
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
        self.local().await?.clients.save_pool(input).await
    }
}

impl AppState {
    pub async fn recover_owner(&self) -> anyhow::Result<AppStatus> {
        let _lock = self.operation.lock().await;
        anyhow::ensure!(
            !self.stopping.load(Ordering::SeqCst),
            "Gate is shutting down"
        );
        anyhow::ensure!(
            self.local.lock().await.is_none(),
            "stop this node before offline owner recovery"
        );
        let identity = Identity::recover(&self.directory, &self.vault).await?;
        self.mount(identity).await?;
        self.journal_failure.store(false, Ordering::SeqCst);
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
    pub async fn configure_paid_serve(
        &self,
        expected: u64,
        input: PaidServeInput,
    ) -> anyhow::Result<ProviderView> {
        let _lock = self.operation.lock().await;
        let local = self.local().await?;
        let session = local.session(true).await?;
        crate::control::check_revision(&session.view().await?, expected)?;
        crate::paid::configure_serve(&local, &session, input).await?;
        let view = session.view().await?;
        crate::provider_setup::view_document(&view.document, view.revision.0)
    }
}

impl AppState {
    pub async fn save_listen_port(&self, port: Option<u16>) -> anyhow::Result<AppStatus> {
        let _lock = self.operation.lock().await;
        anyhow::ensure!(
            !self.stopping.load(Ordering::SeqCst),
            "Gate is shutting down"
        );
        let local = self.local().await?;
        local
            .session(true)
            .await?
            .edit(None, |document| {
                let hellas_sdk::daemon_config::Listener::Iroh { port: current, .. } = document
                    .listeners
                    .get_mut("work")
                    .context("start Serve before editing its port")?
                else {
                    anyhow::bail!("provider listener is not iroh")
                };
                *current = port.unwrap_or(0);
                Ok(())
            })
            .await?;
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
        self.journal_failure.store(false, Ordering::SeqCst);
        *self.failure.lock().await = None;
        self.status().await
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[tokio::test(flavor = "multi_thread")]
    async fn failed_existing_journal_preserves_data_and_disables_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let vault = Vault::fixture(secrets.path());
        let app = AppState::with_vault(directory.path(), vault.clone())
            .await
            .unwrap();
        app.initialize_software("Owner".into()).await.unwrap();
        app.shutdown().await.unwrap();
        drop(app);
        let authority = directory.path().join("node/authority");
        std::fs::remove_file(&authority).unwrap();
        std::fs::write(&authority, b"incompatible test journal").unwrap();
        let installation = std::fs::read(directory.path().join("installation.json")).unwrap();
        let reopened = AppState::with_vault(directory.path(), vault).await.unwrap();
        let status = reopened.status().await.unwrap();
        assert!(status.identity.journal_error);
        assert!(!status.identity.recovery_available);
        assert!(!status.identity.initialized);
        assert_eq!(
            std::fs::read(directory.path().join("installation.json")).unwrap(),
            installation
        );
        assert_eq!(
            std::fs::read(authority).unwrap(),
            b"incompatible test journal"
        );
    }
    #[tokio::test]
    async fn duplicate_setup_does_not_wait_behind_active_operation() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let app = AppState::with_vault(directory.path(), Vault::fixture(secrets.path()))
            .await
            .unwrap();
        let _active = app.operation.lock().await;
        for result in [
            app.initialize_native("Owner".into()).await,
            app.initialize_software("Owner".into()).await,
        ] {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("already in progress")
            );
        }
    }
    #[tokio::test]
    async fn shutdown_closes_admission_before_waiting_for_an_approval() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let app = Arc::new(
            AppState::with_vault(directory.path(), Vault::fixture(secrets.path()))
                .await
                .unwrap(),
        );
        let approval = app.operation.lock().await;
        let task = tokio::spawn({
            let app = app.clone();
            async move { app.shutdown().await }
        });
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !app.stopping.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("shutdown must signal before waiting for the approval lock");
        assert!(!task.is_finished());
        let mut duplicate = Box::pin(app.shutdown());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut duplicate)
                .await
                .is_err(),
            "a repeated shutdown must not return while the first is still draining"
        );
        drop(approval);
        assert!(task.await.unwrap().unwrap());
        assert!(!duplicate.await.unwrap());
        assert!(
            app.initialize_software("Late approval".into())
                .await
                .is_err()
        );
        assert!(app.open_existing().await.is_err());
    }
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
        let recovered = app.recover_owner().await.unwrap();
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
