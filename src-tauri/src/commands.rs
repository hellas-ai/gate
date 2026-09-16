use std::sync::Arc;

use futures::StreamExt;
use tauri::State;
use tauri::ipc::Channel;

use crate::dto::{
    AppStatus, ExecutionEvent, GatewayAccess, HistoryEntry, ProviderConfig, RunRequest,
};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

#[tauri::command]
pub async fn get_status(state: State<'_, Arc<AppState>>) -> ApiResult<AppStatus> {
    Ok(state.status().await)
}

#[tauri::command]
pub async fn set_provider_enabled(
    state: State<'_, Arc<AppState>>,
    enabled: bool,
    config: Option<ProviderConfig>,
) -> ApiResult<AppStatus> {
    state
        .set_provider_enabled(enabled, config)
        .await
        .map_err(ApiError::internal)
}

#[tauri::command]
pub async fn set_gateway_enabled(
    state: State<'_, Arc<AppState>>,
    enabled: bool,
    config: Option<RunRequest>,
) -> ApiResult<AppStatus> {
    state
        .set_gateway_enabled(enabled, config)
        .await
        .map_err(ApiError::internal)
}

#[tauri::command]
pub async fn get_gateway_access(state: State<'_, Arc<AppState>>) -> ApiResult<GatewayAccess> {
    state
        .gateway_access()
        .await
        .ok_or_else(|| ApiError::new("gateway_stopped", "The loopback gateway is not running"))
}

#[tauri::command]
pub async fn run_request(
    state: State<'_, Arc<AppState>>,
    request: RunRequest,
    on_event: Channel<ExecutionEvent>,
) -> ApiResult<String> {
    if request.input.trim().is_empty() {
        return Err(ApiError::new("invalid_request", "Input cannot be empty"));
    }
    if request.target.trim().is_empty() || request.trust_anchor.trim().is_empty() {
        return Err(ApiError::new(
            "trust_required",
            "A target and explicit trust anchor are required",
        ));
    }

    let run_id = state.history.begin(&request).map_err(ApiError::internal)?;
    on_event
        .send(ExecutionEvent::Started {
            run_id: run_id.clone(),
        })
        .map_err(ApiError::internal)?;

    let mut stream = match state.fetch(&request).await {
        Ok(stream) => stream,
        Err(error) => {
            let message = error.to_string();
            state
                .history
                .fail(&run_id, &message)
                .map_err(ApiError::internal)?;
            on_event
                .send(ExecutionEvent::Failed {
                    run_id: run_id.clone(),
                    message,
                })
                .map_err(ApiError::internal)?;
            return Ok(run_id);
        }
    };

    while let Some(event) = stream.next().await {
        match event {
            Ok(hellas_sdk::client::FetchExecutionEvent::Chunk { event, .. }) => {
                let record = serde_json::to_string(&event).map_err(ApiError::internal)?;
                let text = render_output_event(&event).map_err(ApiError::internal)?;
                state
                    .history
                    .append_result(&run_id, &record)
                    .map_err(ApiError::internal)?;
                on_event
                    .send(ExecutionEvent::Output { text })
                    .map_err(ApiError::internal)?;
            }
            Ok(hellas_sdk::client::FetchExecutionEvent::Done(
                hellas_sdk::client::FetchOutcome::Completed { terminal, .. },
            )) => {
                let text = serde_json::to_string(&terminal.to_output_event())
                    .map_err(ApiError::internal)?;
                let verification =
                    "Provider identity, signatures, commitments, and Fetch transcript verified";
                state
                    .history
                    .append_result(&run_id, &text)
                    .and_then(|()| state.history.complete(&run_id, verification))
                    .map_err(ApiError::internal)?;
                on_event
                    .send(ExecutionEvent::Output { text })
                    .and_then(|()| {
                        on_event.send(ExecutionEvent::Verification {
                            summary: verification.into(),
                        })
                    })
                    .and_then(|()| {
                        on_event.send(ExecutionEvent::Finished {
                            run_id: run_id.clone(),
                        })
                    })
                    .map_err(ApiError::internal)?;
                return Ok(run_id);
            }
            Ok(hellas_sdk::client::FetchExecutionEvent::Done(
                hellas_sdk::client::FetchOutcome::Failed { position, error },
            )) => {
                let message = format!("Fetch failed at byte {position}: {error}");
                state
                    .history
                    .fail(&run_id, &message)
                    .map_err(ApiError::internal)?;
                on_event
                    .send(ExecutionEvent::Failed {
                        run_id: run_id.clone(),
                        message,
                    })
                    .map_err(ApiError::internal)?;
                return Ok(run_id);
            }
            Err(error) => {
                let message = error.to_string();
                state
                    .history
                    .fail(&run_id, &message)
                    .map_err(ApiError::internal)?;
                on_event
                    .send(ExecutionEvent::Failed {
                        run_id: run_id.clone(),
                        message,
                    })
                    .map_err(ApiError::internal)?;
                return Ok(run_id);
            }
        }
    }

    let message = "Fetch stream ended without a terminal outcome";
    state
        .history
        .fail(&run_id, message)
        .map_err(ApiError::internal)?;
    on_event
        .send(ExecutionEvent::Failed {
            run_id: run_id.clone(),
            message: message.into(),
        })
        .map_err(ApiError::internal)?;
    Ok(run_id)
}

fn render_output_event(event: &hellas_rpc::output::OutputEvent) -> serde_json::Result<String> {
    match event {
        hellas_rpc::output::OutputEvent::TextDelta { delta, .. } => Ok(delta.clone()),
        _ => Ok(format!("{}\n", serde_json::to_string(event)?)),
    }
}

#[tauri::command]
pub async fn list_history(
    state: State<'_, Arc<AppState>>,
    limit: Option<u32>,
) -> ApiResult<Vec<HistoryEntry>> {
    state
        .history
        .list(limit.unwrap_or(100))
        .map_err(ApiError::internal)
}

#[tauri::command]
pub async fn delete_history(state: State<'_, Arc<AppState>>, id: String) -> ApiResult<bool> {
    state.history.delete(&id).map_err(ApiError::internal)
}

#[tauri::command]
pub async fn clear_history(state: State<'_, Arc<AppState>>) -> ApiResult<usize> {
    state.history.clear().map_err(ApiError::internal)
}
