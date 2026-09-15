use serde::{Deserialize, Serialize};
#[cfg(feature = "desktop")]
use ts_rs::TS;

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub state: ServiceState,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ServiceState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct IdentityStatus {
    pub producer_id: String,
    pub caller_public_key: String,
    pub node_id: String,
    pub attestation: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub version: String,
    pub identity: IdentityStatus,
    pub provider: ServiceStatus,
    pub gateway: ServiceStatus,
    pub endpoint: String,
    pub socket_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct RunRequest {
    pub kind: RunKind,
    pub target: String,
    #[serde(default)]
    pub node_addresses: Vec<String>,
    pub input: String,
    pub trust_anchor: String,
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub execution_environment: String,
    #[serde(default)]
    pub assurance: AssuranceInput,
    #[serde(default)]
    pub apple_app_id: String,
    #[serde(default)]
    pub apple_cd_hashes: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub enum RunKind {
    CausalLm,
    Fetch,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub enum AssuranceInput {
    #[default]
    ProducerSigned,
    AppleAppAttest,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(
    tag = "type",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ExecutionEvent {
    Started { run_id: String },
    Output { text: String },
    Verification { summary: String },
    Finished { run_id: String },
    Failed { run_id: String, message: String },
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub created_at_ms: i64,
    pub kind: String,
    pub target: String,
    pub status: String,
    pub request: String,
    pub result: String,
    pub verification: String,
    pub error: String,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct GatewayAccess {
    pub address: String,
    pub bearer: String,
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub service: String,
    pub method: String,
    pub openai_api_key: String,
    #[serde(default)]
    pub allowed_callers: Vec<String>,
    #[cfg_attr(feature = "desktop", ts(optional))]
    pub port: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::ExecutionEvent;

    #[test]
    fn execution_events_expose_the_run_id_expected_by_the_ui() {
        for event in [
            ExecutionEvent::Started {
                run_id: "run-1".into(),
            },
            ExecutionEvent::Finished {
                run_id: "run-1".into(),
            },
            ExecutionEvent::Failed {
                run_id: "run-1".into(),
                message: "failed".into(),
            },
        ] {
            let value = serde_json::to_value(event).unwrap();
            assert_eq!(value["data"]["runId"], "run-1");
            assert!(value["data"].get("run_id").is_none());
        }
    }
}
