use serde::{Deserialize, Serialize};
#[cfg(feature = "desktop")]
use ts_rs::TS;

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct IdentityStatus {
    pub initialized: bool,
    pub principal: String,
    pub fingerprint: String,
    pub node_key: String,
    pub transport_id: String,
    pub authority: String,
    pub generation: u64,
    pub attestation: String,
    pub detail: String,
    pub contact_card: String,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub state: String,
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
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub enum RunKind {
    Fetch,
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
        access_id: String,
        allowance: String,
        resource: String,
    },
    Paid {
        pool_id: String,
        provider: String,
        service: String,
        method: String,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkClientConfig {
    pub target: WorkTarget,
    pub trust_policy: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunRequest {
    pub kind: RunKind,
    pub input: String,
    pub client: WorkClientConfig,
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
    Cancelled { run_id: String },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustInput {
    pub id: String,
    pub expected_revision: u64,
    pub genesis: String,
    pub apple: bool,
    pub environments: Vec<String>,
    pub app_id: String,
    pub cd_hashes: Vec<String>,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TrustRecord {
    pub id: String,
    pub revision: u64,
    pub genesis: String,
    pub assurance: String,
    pub environments: Vec<String>,
    pub source: String,
}
