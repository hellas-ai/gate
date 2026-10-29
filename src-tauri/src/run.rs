//! Run consumes the same SDK clients as gateways and records only verified completion.
use crate::{dto::*, state::AppState};
use futures::StreamExt;

pub async fn execute(
    state: &AppState,
    request: RunRequest,
    send: impl Fn(ExecutionEvent),
) -> anyhow::Result<String> {
    let (local, _permit, id, mut cancelled) = state.begin_run(&request).await?;
    send(ExecutionEvent::Started { run_id: id.clone() });
    let result:anyhow::Result<bool>=async {
        // Keep SDK opening/recovery owned until it resolves. Cancellation cannot
        // abandon its journal recovery or accepted work in another live session.
        let opened=local.clients.fetch(&local,&request).await;
        if *cancelled.borrow() {return Ok(false);}
        let mut stream=opened?;let mut terminal=false;
        loop {
            let next=tokio::select! {biased;_ = cancelled.wait_for(|value|*value)=>{return Ok(false);},next=stream.next()=>next};
            let Some(event)=next else {break;};
            // Run records authenticated output; gateway presentations carry the
            // separate acceptance and transcript metadata.
            let hellas_gateway::WorkEvent::Output(event)=event? else {continue;};
            let text=serde_json::to_string(&event)?;state.history.append_result(&id,&text)?;
            let success=event.terminal();
            let failure=(success==Some(false)).then(||format!("Fetch returned an unsuccessful terminal: {text}"));
            send(ExecutionEvent::Output{text});
            if let Some(failure)=failure {anyhow::bail!(failure);}
            terminal|=success==Some(true);
        }
        anyhow::ensure!(terminal,"Fetch stream ended without a verified terminal event");Ok(true)
    }.await;
    state.cancellations.lock().await.remove(&id);
    match result {
        Ok(true) => {
            let summary = "SDK verified the signed terminal result and output stream";
            state.history.complete(&id, summary)?;
            send(ExecutionEvent::Verification {
                summary: summary.into(),
            });
            send(ExecutionEvent::Finished { run_id: id.clone() });
        }
        Ok(false) => {
            state.history.cancel(&id)?;
            send(ExecutionEvent::Cancelled { run_id: id.clone() });
        }
        Err(error) => {
            state.history.fail(&id, &error.to_string())?;
            send(ExecutionEvent::Failed {
                run_id: id.clone(),
                message: error.to_string(),
            });
        }
    }
    Ok(id)
}
