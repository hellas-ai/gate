use crate::daemon::inner::DaemonInner;
use crate::daemon::rpc::DaemonRequest;
use tokio::sync::mpsc;

pub struct DaemonActor {
    inner: DaemonInner,
    rx: mpsc::Receiver<DaemonRequest>,
}

impl DaemonActor {
    pub fn new(inner: DaemonInner, rx: mpsc::Receiver<DaemonRequest>) -> Self {
        Self { inner, rx }
    }

    pub async fn run(mut self) {
        while let Some(req) = self.rx.recv().await {
            match req {
                DaemonRequest::GetStatus { reply } => {
                    let _ = reply.send(self.inner.status().await);
                }
                DaemonRequest::UpdateConfig {
                    identity,
                    config,
                    reply,
                } => {
                    let result = self.inner.update_config(&identity, *config).await;
                    let _ = reply.send(result);
                }
                DaemonRequest::Restart { identity, reply } => {
                    let result = self.inner.restart(&identity).await;
                    let _ = reply.send(result);
                }
                DaemonRequest::Shutdown { identity, reply } => {
                    let result = self.inner.shutdown(&identity).await;
                    let should_break = result.is_ok();
                    let _ = reply.send(result);
                    if should_break {
                        break;
                    }
                }
                DaemonRequest::GetSettings { reply } => {
                    let _ = reply.send(self.inner.get_settings().await);
                }
                DaemonRequest::GetBootstrapManager { reply } => {
                    let _ = reply.send(self.inner.get_bootstrap_manager());
                }
                DaemonRequest::GetWebAuthnService { reply } => {
                    let _ = reply.send(self.inner.get_webauthn_service());
                }
                DaemonRequest::GetPermissionManager { reply } => {
                    let _ = reply.send(self.inner.get_permission_manager());
                }
                DaemonRequest::GetAuthService { reply } => {
                    let _ = reply.send(self.inner.get_auth_service());
                }
                DaemonRequest::GetStateBackend { reply } => {
                    let _ = reply.send(self.inner.get_state_backend());
                }
                DaemonRequest::GetUpstreamRegistry { reply } => {
                    let _ = reply.send(self.inner.get_upstream_registry());
                }
                DaemonRequest::GetInferenceBackend { reply } => {
                    let _ = reply.send(self.inner.get_inference_backend());
                }
                DaemonRequest::GetUserCount { reply } => {
                    let _ = reply.send(self.inner.get_user_count());
                }
                DaemonRequest::GetConfig { identity, reply } => {
                    let result = self.inner.get_config(&identity).await;
                    let _ = reply.send(result);
                }
            }
        }
    }
}
