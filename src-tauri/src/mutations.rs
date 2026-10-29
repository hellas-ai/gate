//! Acknowledged operations for an apply report, never authority or recovery state.
use crate::control::Session;
use hellas_authority::model::grant::admin::{WorkControlCommand, WorkControlReply};

#[derive(Default)]
pub(crate) struct Progress {
    pub completed: Vec<String>,
    pub pending: Option<(String, bool)>,
}
impl Session {
    fn begin_mutation(&self, label: String, atomic_rpc: bool) -> anyhow::Result<()> {
        let mut progress = self
            .progress
            .lock()
            .map_err(|_| anyhow::anyhow!("operation receipt lock failed"))?;
        anyhow::ensure!(
            progress.pending.is_none(),
            "an earlier mutation has not been reconciled"
        );
        progress.pending = Some((label, atomic_rpc));
        Ok(())
    }
    fn finish_mutation(&self) -> anyhow::Result<()> {
        let mut progress = self
            .progress
            .lock()
            .map_err(|_| anyhow::anyhow!("operation receipt lock failed"))?;
        if let Some((label, _)) = progress.pending.take() {
            progress.completed.push(label);
        }
        Ok(())
    }
    pub(crate) fn take_progress(&self) -> anyhow::Result<Progress> {
        let mut progress = self
            .progress
            .lock()
            .map_err(|_| anyhow::anyhow!("operation receipt lock failed"))?;
        Ok(std::mem::take(&mut *progress))
    }
    pub(crate) async fn track<T, E>(
        &self,
        label: String,
        atomic_rpc: bool,
        operation: impl std::future::Future<Output = Result<T, E>>,
    ) -> anyhow::Result<T>
    where
        E: Into<anyhow::Error>,
    {
        self.begin_mutation(label, atomic_rpc)?;
        let result = operation.await.map_err(Into::into)?;
        self.finish_mutation()?;
        Ok(result)
    }
    pub(crate) fn track_local<T>(
        &self,
        label: String,
        operation: impl FnOnce() -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        self.begin_mutation(label, false)?;
        let result = operation()?;
        self.finish_mutation()?;
        Ok(result)
    }
    pub(crate) async fn work(
        &self,
        command: WorkControlCommand,
    ) -> anyhow::Result<WorkControlReply> {
        let read = matches!(
            &command,
            WorkControlCommand::Catalogue
                | WorkControlCommand::InspectBudget { .. }
                | WorkControlCommand::List
                | WorkControlCommand::Inspect { .. }
                | WorkControlCommand::Offers { .. }
                | WorkControlCommand::InspectResource { .. }
        );
        let operation =
            crate::control::work(&self.client, self.authorization.clone(), command.clone());
        if read {
            operation.await
        } else {
            self.track(format!("Work {command:?}"), true, operation)
                .await
        }
    }
    pub(crate) async fn provider_state(&self, enabled: bool) -> anyhow::Result<()> {
        self.track(
            format!("Set provider enabled={enabled}"),
            false,
            self.client
                .set_provider_state(hellas_rpc::pb::host::SetProviderStateRequest {
                    authorization: self.authorization.clone(),
                    enabled,
                }),
        )
        .await?;
        Ok(())
    }
    pub(crate) async fn gateway_state(&self, enabled: bool) -> anyhow::Result<()> {
        self.track(
            format!("Set gateway enabled={enabled}"),
            false,
            self.client
                .set_gateway_state(hellas_rpc::pb::host::SetGatewayStateRequest {
                    authorization: self.authorization.clone(),
                    enabled,
                }),
        )
        .await?;
        Ok(())
    }
}
