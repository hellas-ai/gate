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
