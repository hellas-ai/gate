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
    pub contact: String,
    pub contact_id: String,
    pub apple_app_id: String,
    pub apple_cd_hashes: Vec<String>,
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
    pub target: WorkTarget,
    pub input: String,
    #[serde(default)]
    pub assurance: AssuranceInput,
    #[serde(default)]
    pub apple_app_id: String,
    #[serde(default)]
    pub apple_cd_hashes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(
    tag = "funding",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkTarget {
    Authorized {
        offer: String,
        resource: String,
    },
    Paid {
        pool_config: String,
        provider: String,
        #[cfg_attr(feature = "desktop", ts(optional))]
        route: Option<FetchRouteInput>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FetchRouteInput {
    pub service: String,
    pub method: String,
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
    #[cfg_attr(feature = "desktop", ts(optional))]
    pub work_config_path: Option<String>,
    #[cfg_attr(feature = "desktop", ts(optional))]
    pub https_config: Option<String>,
    #[serde(default)]
    pub contacts: Vec<String>,
    pub requests_per_day: u32,
    #[cfg_attr(feature = "desktop", ts(optional))]
    pub port: Option<u16>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ProviderOffer {
    pub contact: String,
    pub offer: String,
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
