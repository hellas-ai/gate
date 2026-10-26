use serde::{Deserialize, Serialize};
#[cfg(feature = "desktop")]
use ts_rs::TS;

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct IdentityStatus {
    pub initialized: bool,
    pub recovery_available: bool,
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
    pub listen_port: Option<u16>,
    pub bound_addresses: Vec<String>,
    pub min_omit_response_blocks: u64,
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ResourceKind {
    OpenAi {
        model: String,
        max_output_tokens: u32,
        credential: String,
    },
    Https {
        origin: String,
        paths: Vec<String>,
        methods: Vec<String>,
        credential: Option<String>,
        max_response_bytes: u32,
        tls_roots: Vec<String>,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceInput {
    pub name: String,
    pub service: String,
    pub method: String,
    pub resource: ResourceKind,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpstreamKeyInput {
    pub service: String,
    pub method: String,
    pub key: String,
    pub origins: Vec<String>,
    pub paths: Vec<String>,
    pub methods: Vec<String>,
    pub header: String,
    pub prefix: String,
    pub secret: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProviderChange {
    Endpoint {
        service: String,
        method: String,
        hosts: Vec<String>,
        allow_private_addresses: bool,
    },
    RemoveEndpoint {
        service: String,
        method: String,
    },
    AddKey {
        key: UpstreamKeyInput,
    },
    ReplaceKey {
        key: UpstreamKeyInput,
    },
    RemoveKey {
        service: String,
        method: String,
        key: String,
    },
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct UpstreamView {
    pub service: String,
    pub method: String,
    pub hosts: Vec<String>,
    pub keys: Vec<UpstreamKeyView>,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub revision: u64,
    pub upstreams: Vec<UpstreamView>,
    pub resources: Vec<ResourceInput>,
    pub paid: Option<PaidServeInput>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContactInput {
    pub card: String,
    pub holder: String,
    pub confirmed_holder: bool,
    pub label: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContactView {
    pub id: String,
    pub fingerprint: String,
    pub name: String,
    pub label: String,
    pub devices: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AllowanceInput {
    pub resource: String,
    pub requests_per_day: u64,
    pub max_in_flight: u16,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShareInput {
    pub request_id: String,
    pub contact: String,
    pub holder: String,
    pub allowances: Vec<AllowanceInput>,
    pub period: crate::period::Period,
    pub label: String,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ShareView {
    pub id: String,
    pub contact: String,
    pub holder: String,
    pub label: String,
    pub expires: u64,
    pub allowances: Vec<String>,
    pub credential: String,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AccessView {
    pub id: String,
    pub label: String,
    pub expires: u64,
    pub allowances: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenesisValidatorInput {
    pub public_key: String,
    pub label: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenesisAllocationInput {
    pub address: String,
    pub balance: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChainInput {
    pub network: String,
    pub genesis_digest: String,
    pub threshold_identity: String,
    pub validators: Vec<String>,
    pub committee: Vec<GenesisValidatorInput>,
    pub allocations: Vec<GenesisAllocationInput>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub enum FetchPresentation {
    Responses,
    Http,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaidPolicyInput {
    pub chain: ChainInput,
    pub service: String,
    pub method: String,
    pub presentation: FetchPresentation,
    pub policy_salt: String,
    pub compute_credit_limit: u64,
    pub delivery_credit_limit: u64,
    pub fixed_price: u64,
    pub dispatch_blocks: u64,
    pub delivery_blocks: u64,
    pub oracle_blocks: u64,
    pub value: u64,
    pub reserve: u64,
    pub fee_base: u64,
    pub fee_slot: u64,
    pub fee_proof: u64,
    pub fee_lifetime: u64,
    pub min_omit_response_blocks: u64,
    pub poll_ms: u64,
    pub max_observation_age_ms: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaidProviderInput {
    pub provider: String,
    pub addresses: Vec<String>,
    pub trust_policy: String,
    pub bond: String,
    pub coins: Vec<String>,
    pub omission_bond: u64,
    pub acceptance_blocks: u64,
    pub terminal_blocks: u64,
    pub payment_blocks: u64,
    pub timeout_seconds: u64,
    pub policy: PaidPolicyInput,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaidPoolInput {
    pub id: String,
    pub providers: Vec<PaidProviderInput>,
    pub max_pending_requests: usize,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HttpRouteInput {
    pub path: String,
    pub method: String,
    pub url: String,
    pub credential: Option<String>,
    pub max_response_bytes: u32,
    pub tls_roots: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayConfig {
    pub client: WorkClientConfig,
    pub http_routes: Vec<HttpRouteInput>,
    pub port: Option<u16>,
    pub zdr: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayKeyInput {
    pub request_id: String,
    pub label: String,
    pub period: crate::period::Period,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct GatewayKeyView {
    pub id: String,
    pub label: String,
    pub expires: u64,
    pub state: GatewayKeyState,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub enum GatewayKeyState {
    Pending,
    Active,
    Revoking,
    Revoked,
    Expired,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct UpstreamKeyView {
    pub secret_reference: String,
    pub name: String,
    pub fingerprint: String,
    pub last_use: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdminAccessInput {
    pub request_id: String,
    pub label: String,
    pub holder: String,
    pub period: crate::period::Period,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AdminAccess {
    pub authority: String,
    pub authorization: String,
    pub holder: String,
    pub expires: u64,
    pub socket_path: String,
    pub certificate_base64: String,
    pub server_name: String,
    pub credential_hex: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesiredKey {
    pub name: String,
    pub origins: Vec<String>,
    pub paths: Vec<String>,
    pub methods: Vec<String>,
    pub header: String,
    pub prefix: String,
    pub secret_reference: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesiredUpstream {
    pub service: String,
    pub method: String,
    pub hosts: Vec<String>,
    pub allow_private_addresses: bool,
    pub keys: Vec<DesiredKey>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuotaInput {
    pub allowance: String,
    pub expected_revision: u64,
    pub requests_per_day: u64,
    pub max_in_flight: u16,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesiredState {
    pub schema: u32,
    pub expected_provider_revision: u64,
    pub upstreams: Vec<DesiredUpstream>,
    pub resources: Vec<ResourceInput>,
    pub contacts: Vec<ContactInput>,
    pub shares: Vec<ShareInput>,
    pub gateway_keys: Vec<GatewayKeyInput>,
    pub revoke_shares: Vec<String>,
    pub revoke_gateway_keys: Vec<String>,
    pub quotas: Vec<QuotaInput>,
    pub provider_enabled: Option<bool>,
    pub gateway: Option<GatewayConfig>,
    pub gateway_enabled: Option<bool>,
    #[serde(default)]
    pub paid_serve: Option<DesiredPaidServe>,
    #[serde(default)]
    pub paid_pools: Vec<PaidPoolInput>,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ApplyPreview {
    pub id: String,
    pub provider_revision: u64,
    pub steps: Vec<String>,
    pub requires_passkey: bool,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    pub applied: Vec<String>,
    pub unapplied: Vec<String>,
    pub uncertain: Vec<String>,
    pub error: Option<String>,
    pub provider_revision: u64,
    pub authority_revision: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct QuotaView {
    pub revision: u64,
    pub requests_per_day: Option<u64>,
    pub max_in_flight: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaidRouteInput {
    pub peer: String,
    pub bond: String,
    pub client: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaidServeInput {
    pub policy: PaidPolicyInput,
    pub routes: Vec<PaidRouteInput>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProvisionInput {
    pub expected_provider_revision: u64,
    pub contact: String,
    pub client_node: String,
    pub client_peer: String,
    pub addresses: Vec<String>,
    pub redelegate_payout: bool,
    pub stake_coins: Vec<String>,
    pub bond_timeout: u64,
    pub timeout_payout: u64,
    pub max_job_price: u64,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ProvisionView {
    pub bond: String,
    pub owner: String,
    pub provider_node: String,
    pub client: String,
    pub client_node: String,
    pub payout_delegate: Option<String>,
    pub timeout_payout: u64,
    pub bond_timeout: u64,
    pub stake_coins: Vec<String>,
    pub signed_offer: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "desktop", derive(TS))]
#[cfg_attr(feature = "desktop", ts(export))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesiredPaidServe {
    pub configuration: Option<PaidServeInput>,
}
