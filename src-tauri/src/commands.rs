use crate::{
    dto::*,
    error::{ApiError, ApiResult},
    state::AppState,
};
use std::sync::Arc;
use tauri::State;
#[tauri::command]
pub async fn get_status(state: State<'_, Arc<AppState>>) -> ApiResult<AppStatus> {
    state.status().await.map_err(ApiError::internal)
}
#[tauri::command]
pub async fn initialize_software(
    state: State<'_, Arc<AppState>>,
    name: String,
) -> ApiResult<AppStatus> {
    state
        .initialize_software(name)
        .await
        .map_err(ApiError::internal)
}
#[tauri::command]
pub fn list_history(state: State<'_, Arc<AppState>>, limit: u32) -> ApiResult<Vec<HistoryEntry>> {
    state.history.list(limit).map_err(ApiError::internal)
}
#[tauri::command]
pub fn delete_history(state: State<'_, Arc<AppState>>, id: String) -> ApiResult<bool> {
    state.history.delete(&id).map_err(ApiError::internal)
}
#[tauri::command]
pub fn clear_history(state: State<'_, Arc<AppState>>) -> ApiResult<usize> {
    state.history.clear().map_err(ApiError::internal)
}
#[tauri::command]
pub async fn list_trust(state: State<'_, Arc<AppState>>) -> ApiResult<Vec<TrustRecord>> {
    state.local().await.map_err(ApiError::internal)?;
    crate::trust::list(&state.directory).map_err(ApiError::internal)
}
#[tauri::command]
pub async fn approve_trust(
    state: State<'_, Arc<AppState>>,
    input: TrustInput,
) -> ApiResult<TrustRecord> {
    state.local().await.map_err(ApiError::internal)?;
    crate::trust::approve(&state.directory, input).map_err(ApiError::internal)
}
#[tauri::command]
pub async fn bind_provider(
    state: State<'_, Arc<AppState>>,
    software: bool,
) -> ApiResult<AppStatus> {
    state
        .bind_provider(software)
        .await
        .map_err(ApiError::internal)
}
#[tauri::command]
pub async fn configure_provider(
    state: State<'_, Arc<AppState>>,
    expected: u64,
    change: ProviderChange,
) -> ApiResult<ProviderView> {
    state
        .configure_provider(expected, change)
        .await
        .map_err(ApiError::internal)
}
#[tauri::command]
pub async fn provider_view(state: State<'_, Arc<AppState>>) -> ApiResult<ProviderView> {
    state.provider_view().await.map_err(ApiError::internal)
}
#[tauri::command]
pub async fn set_provider(state: State<'_, Arc<AppState>>, enabled: bool) -> ApiResult<AppStatus> {
    state
        .set_provider(enabled)
        .await
        .map_err(ApiError::internal)
}
#[tauri::command]
pub async fn configure_resource(
    state: State<'_, Arc<AppState>>,
    expected: u64,
    input: ResourceInput,
) -> ApiResult<ProviderView> {
    state
        .configure_resource(expected, input)
        .await
        .map_err(ApiError::internal)
}
#[tauri::command]
pub async fn list_contacts(state: State<'_, Arc<AppState>>) -> ApiResult<Vec<ContactView>> {
    state.contacts().await.map_err(ApiError::internal)
}
#[tauri::command]
pub async fn add_contact(
    state: State<'_, Arc<AppState>>,
    input: ContactInput,
) -> ApiResult<ContactView> {
    state.add_contact(input).await.map_err(ApiError::internal)
}
#[tauri::command]
pub async fn list_shares(state: State<'_, Arc<AppState>>) -> ApiResult<Vec<ShareView>> {
    state.shares().await.map_err(ApiError::internal)
}
#[tauri::command]
pub async fn share_access(
    state: State<'_, Arc<AppState>>,
    input: ShareInput,
) -> ApiResult<ShareView> {
    state.share(input).await.map_err(ApiError::internal)
}
#[tauri::command]
pub async fn export_access(state: State<'_, Arc<AppState>>, id: String) -> ApiResult<String> {
    state.export_access(&id).await.map_err(ApiError::internal)
}
#[tauri::command]
pub async fn import_access(
    state: State<'_, Arc<AppState>>,
    bundle: String,
) -> ApiResult<AccessView> {
    state
        .import_access(&bundle)
        .await
        .map_err(ApiError::internal)
}
#[tauri::command]
pub async fn list_access(state: State<'_, Arc<AppState>>) -> ApiResult<Vec<AccessView>> {
    state.accesses().await.map_err(ApiError::internal)
}
#[tauri::command]
pub async fn revoke_share(state: State<'_, Arc<AppState>>, id: String) -> ApiResult<()> {
    state.revoke_share(&id).await.map_err(ApiError::internal)
}
#[tauri::command]
pub async fn pause_allowance(
    state: State<'_, Arc<AppState>>,
    id: String,
    paused: bool,
) -> ApiResult<()> {
    state
        .pause_allowance(&id, paused)
        .await
        .map_err(ApiError::internal)
}
#[tauri::command]
pub async fn set_gateway(
    state: State<'_, Arc<AppState>>,
    enabled: bool,
    config: Option<GatewayConfig>,
) -> ApiResult<AppStatus> {
    state
        .set_gateway(enabled, config)
        .await
        .map_err(ApiError::internal)
}
#[tauri::command]
pub async fn gateway_address(state: State<'_, Arc<AppState>>) -> ApiResult<String> {
    state.gateway_address().await.map_err(ApiError::internal)
}
#[tauri::command]
pub async fn list_gateway_keys(state: State<'_, Arc<AppState>>) -> ApiResult<Vec<GatewayKeyView>> {
    state.gateway_keys().await.map_err(ApiError::internal)
}
#[tauri::command]
pub async fn issue_gateway_key(
    state: State<'_, Arc<AppState>>,
    input: GatewayKeyInput,
) -> ApiResult<(GatewayKeyView, String)> {
    state
        .issue_gateway_key(input)
        .await
        .map_err(ApiError::internal)
}
#[tauri::command]
pub async fn revoke_gateway_key(state: State<'_, Arc<AppState>>, id: String) -> ApiResult<()> {
    state
        .revoke_gateway_key(&id)
        .await
        .map_err(ApiError::internal)
}
#[tauri::command]
pub async fn save_paid_pool(
    state: State<'_, Arc<AppState>>,
    input: PaidPoolInput,
) -> ApiResult<()> {
    state
        .save_paid_pool(input)
        .await
        .map_err(ApiError::internal)
}
#[tauri::command]
pub async fn cancel_run(state: State<'_, Arc<AppState>>, id: String) -> ApiResult<()> {
    if let Some(cancel) = state.cancellations.lock().await.get(&id) {
        cancel.send_replace(true);
    }
    Ok(())
}
#[tauri::command]
pub async fn run_request(
    state: State<'_, Arc<AppState>>,
    request: RunRequest,
    on_event: tauri::ipc::Channel<ExecutionEvent>,
) -> ApiResult<String> {
    crate::run::execute(&state, request, |event| {
        let _ = on_event.send(event);
    })
    .await
    .map_err(ApiError::internal)
}

#[tauri::command]
pub async fn recover_software(state: State<'_, Arc<AppState>>) -> ApiResult<AppStatus> {
    state.recover_software().await.map_err(ApiError::internal)
}

#[tauri::command]
pub async fn issue_admin_access(
    state: State<'_, Arc<AppState>>,
    input: AdminAccessInput,
) -> ApiResult<AdminAccess> {
    state
        .issue_admin_access(input)
        .await
        .map_err(ApiError::internal)
}
#[tauri::command]
pub async fn rotate_gateway_key(
    state: State<'_, Arc<AppState>>,
    old: String,
    input: GatewayKeyInput,
) -> ApiResult<(GatewayKeyView, String)> {
    state
        .rotate_gateway_key(&old, input)
        .await
        .map_err(ApiError::internal)
}

#[tauri::command]
pub async fn preview_apply(
    state: State<'_, Arc<AppState>>,
    document: DesiredState,
) -> ApiResult<ApplyPreview> {
    state
        .preview_apply(document)
        .await
        .map_err(ApiError::internal)
}
#[tauri::command]
pub async fn apply_document(state: State<'_, Arc<AppState>>, id: String) -> ApiResult<ApplyReport> {
    state.apply_document(&id).await.map_err(ApiError::internal)
}
#[tauri::command]
pub async fn set_quota(state: State<'_, Arc<AppState>>, input: QuotaInput) -> ApiResult<()> {
    state.set_quota(input).await.map_err(ApiError::internal)
}

#[tauri::command]
pub async fn get_quota(state: State<'_, Arc<AppState>>, id: String) -> ApiResult<QuotaView> {
    state.quota(&id).await.map_err(ApiError::internal)
}
#[tauri::command]
pub async fn discard_apply(state: State<'_, Arc<AppState>>, id: String) -> ApiResult<()> {
    state.discard_apply(&id).await;
    Ok(())
}

#[tauri::command]
pub async fn reconcile_gateway_key(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> ApiResult<(GatewayKeyView, String)> {
    state
        .reconcile_gateway_key(&id)
        .await
        .map_err(ApiError::internal)
}
