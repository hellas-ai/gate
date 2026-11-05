//! Typed route composition. HostHandle owns every provider start, stop and drain.
use crate::{
    control::LocalNode,
    dto::*,
    provider_identity::ProviderIdentity,
    vault::{self, Vault},
};
use anyhow::{Context, ensure};
use hellas_auth::{
    Digest,
    authority::{ResourceId, Revision},
};
use hellas_authority::model::grant::admin::WorkControlCommand;
use hellas_authority::model::grant::{
    pricing::Pricing, records::GrantPolicy, resource::HttpsResource, responses::ResponsesResource,
};
use hellas_providers::{
    HttpCredential, HttpEgressPolicy, HttpFetchAdaptorFactory, HttpFetchProvider,
};
use hellas_rpc::protocol::invocation::{
    Admission, InvocationPolicy, Limits,
    fetch::{FetchAdmission, FetchLimits, FetchRoutePolicy as WorkRoute},
};
use hellas_sdk::{
    FetchRoute, FetchRouteEntry, FetchRoutePolicy, FetchRouteRegistry,
    grant_config::ResourceFile,
    node_control::{NodeControl, configuration::*, host::*},
};
use std::{
    collections::BTreeMap,
    num::NonZeroU64,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
struct Configuration {
    revision: u64,
    paid: Option<PaidServeInput>,
    policy_revisions: BTreeMap<String, u64>,
    endpoints: BTreeMap<String, Endpoint>,
    resources: Vec<ResourceInput>,
}
#[derive(Clone)]
struct Endpoint {
    service: String,
    method: String,
    hosts: Vec<String>,
    private: bool,
    keys: BTreeMap<String, Key>,
}
#[derive(Clone)]
struct Key {
    origins: Vec<String>,
    paths: Vec<String>,
    methods: Vec<String>,
    header: String,
    prefix: String,
    reference: String,
    fingerprint: String,
    last_use: Option<u64>,
}
#[derive(Clone)]
pub struct Composition {
    directory: PathBuf,
    config_path: PathBuf,
    vault: Vault,
    state: Arc<Mutex<Configuration>>,
    sources: Arc<Mutex<BTreeMap<String, String>>>,
    #[cfg(test)]
    pub(crate) responses_origin: Arc<Mutex<Option<String>>>,
}
fn route_id(service: &str, method: &str) -> String {
    format!("{service}/{method}")
}
fn id(domain: &str, name: &str) -> ResourceId {
    ResourceId(hellas_auth::hash_tuple(domain, &[name.as_bytes()]))
}
impl Composition {
    pub fn open(directory: &Path, vault: Vault) -> anyhow::Result<Self> {
        let config_path = document::path(directory)?;
        if !config_path.try_exists()? && std::env::var_os("HELLAS_GATE_CONFIG").is_none() {
            ensure!(
                !directory.join("provider-configuration.json").try_exists()?,
                "old Gate composition is unsupported; write serve.json using the SDK v1 schema"
            );
            vault::write(
                &config_path,
                &hellas_sdk::serve_config::ServeConfig::empty(),
            )?;
        }
        let config = document::view(&document::read(&config_path)?, directory)?;
        Ok(Self {
            directory: directory.to_owned(),
            config_path,
            vault,
            state: Arc::new(Mutex::new(config)),
            sources: Arc::new(Mutex::new(BTreeMap::new())),
            #[cfg(test)]
            responses_origin: Default::default(),
        })
    }
    fn snapshot(&self) -> anyhow::Result<Configuration> {
        let fresh = document::view(&document::read(&self.config_path)?, &self.directory)?;
        *self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("provider configuration lock failed"))? = fresh.clone();
        Ok(fresh)
    }
    fn publish(&self, expected: u64, mut config: Configuration) -> anyhow::Result<()> {
        let mut held = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("provider configuration lock failed"))?;
        ensure!(
            held.revision == expected,
            "provider revision conflict: expected {}, current {}",
            expected,
            held.revision
        );
        for endpoint in config.endpoints.values_mut() {
            for key in endpoint.keys.values_mut() {
                for previous in held
                    .endpoints
                    .values()
                    .flat_map(|e| e.keys.values())
                    .filter(|previous| {
                        previous.reference == key.reference
                            && previous.fingerprint == key.fingerprint
                    })
                {
                    key.last_use = key.last_use.max(previous.last_use);
                }
            }
        }
        document::write(self, &config, &held)?;
        *held = config;
        Ok(())
    }
    pub fn policy(&self, name: &str) -> anyhow::Result<GrantPolicy> {
        let doc = document::read(&self.config_path)?;
        let resource = doc
            .grants
            .into_iter()
            .flat_map(|g| g.resources)
            .find(|p| p.name == name)
            .context("resource is not in the composition catalogue")?;
        Ok(document::policy(&resource))
    }
    pub fn listen_port(&self) -> anyhow::Result<Option<u16>> {
        Ok(document::read(&self.config_path)?.port)
    }
    pub fn view(&self) -> anyhow::Result<ProviderView> {
        let config = self.snapshot()?;
        Ok(ProviderView {
            revision: config.revision,
            upstreams: config
                .endpoints
                .values()
                .map(|e| UpstreamView {
                    service: e.service.clone(),
                    method: e.method.clone(),
                    hosts: e.hosts.clone(),
                    keys: e
                        .keys
                        .iter()
                        .map(|(name, key)| UpstreamKeyView {
                            name: name.clone(),
                            secret_reference: key.reference.clone(),
                            fingerprint: key.fingerprint.clone(),
                            last_use: key.last_use,
                        })
                        .collect(),
                })
                .collect(),
            resources: config.resources,
            paid: config.paid,
        })
    }
    /// A composition catalogue is not authority; WorkControl must still activate it.
    pub fn resource_revision(
        &self,
        input: &ResourceInput,
        active: &[GrantPolicy],
    ) -> anyhow::Result<(u64, bool)> {
        let config = self.snapshot()?;
        let changed = !config.resources.contains(input);
        if !changed {
            let policy = self.policy(&input.name)?;
            if active.contains(&policy) {
                return Ok((policy.revision.0, false));
            }
        }
        let configured = config
            .resources
            .iter()
            .find(|r| r.name == input.name)
            .map(|_| self.policy(&input.name))
            .transpose()?;
        let resource = configured
            .as_ref()
            .map(|p| p.resource)
            .unwrap_or_else(|| id("hellas.gate.resource.v1", &input.name));
        let account = configured.as_ref().map(|p| p.account).unwrap_or_else(|| {
            id(
                "hellas.gate.account.v1",
                &route_id(&input.service, &input.method),
            )
        });
        let previous = active.iter().find(|policy| policy.resource == resource);
        if let Some(previous) = previous {
            ensure!(
                previous.account == account,
                "a resource cannot move to another upstream account; choose a new resource name"
            );
        }
        let baseline = previous
            .map(|p| p.revision.0)
            .into_iter()
            .chain(configured.as_ref().map(|p| p.revision.0))
            .max();
        let revision = if !changed && previous.is_none() {
            baseline.unwrap_or(1)
        } else {
            baseline
                .map_or(Some(1), |r| r.checked_add(1))
                .context("resource revision overflow")?
        };
        Ok((
            revision,
            changed || config.policy_revisions.get(&input.name) != Some(&revision),
        ))
    }
    pub fn catalogue_many(
        &self,
        expected: u64,
        inputs: Vec<ResourceInput>,
        active: &[GrantPolicy],
    ) -> anyhow::Result<()> {
        let mut config = self.snapshot()?;
        ensure!(config.revision == expected, "provider revision conflict");
        let mut names = std::collections::BTreeSet::new();
        let mut changed = false;
        for input in inputs {
            ensure!(
                names.insert(input.name.clone()),
                "duplicate resource in one change group"
            );
            ensure!(
                config
                    .endpoints
                    .contains_key(&route_id(&input.service, &input.method)),
                "configure this endpoint first"
            );
            let (revision, needs_publication) = self.resource_revision(&input, active)?;
            if !needs_publication {
                continue;
            }
            if let Some(old) = config
                .resources
                .iter()
                .find(|resource| resource.name == input.name)
            {
                ensure!(
                    old.service == input.service && old.method == input.method,
                    "a resource cannot move to another upstream account; choose a new resource name"
                );
            }
            config.policy_revisions.insert(input.name.clone(), revision);
            config
                .resources
                .retain(|resource| resource.name != input.name);
            config.resources.push(input);
            changed = true;
        }
        if !changed {
            return Ok(());
        }
        ensure!(
            config.resources.len() <= 16,
            "at most 16 resources are supported"
        );
        config.revision = config
            .revision
            .checked_add(1)
            .context("provider revision overflow")?;
        self.validate(&config)?;
        self.publish(expected, config)
    }
    pub async fn configure_paid(
        &self,
        local: &LocalNode,
        session: &crate::control::Session,
        expected: u64,
        input: Option<PaidServeInput>,
    ) -> anyhow::Result<ProviderView> {
        let mut next = self.snapshot()?;
        ensure!(next.revision == expected, "provider revision conflict");
        let input = input
            .map(|input| crate::paid::canonical_serve(&self.directory, input))
            .transpose()?;
        next.paid = input;
        self.validate(&next)?;
        let status = session.status().await?;
        let running = status.provider.is_some_and(|p| p.state == 2);
        let changed =
            serde_json::to_vec(&self.snapshot()?.paid)? != serde_json::to_vec(&next.paid)?;
        if let Some(input) = &next.paid {
            ensure!(
                running,
                "start the provider before approving a standing paid policy"
            );
            let policy = crate::paid::standing_policy(&input.policy)?;
            let current = local.coordinator.paid_policy().await?;
            let config = crate::paid::serve_config(&self.directory, input)?;
            local.chain.get(&config).await?;
            if current
                .as_ref()
                .is_none_or(|current| current.policy != policy)
            {
                session
                    .work(WorkControlCommand::SetPaidPolicy {
                        expected_revision: current.map_or(Revision(0), |current| current.revision),
                        policy: Box::new(policy),
                    })
                    .await?;
            }
        }
        if !changed {
            return self.view();
        }
        next.revision = expected
            .checked_add(1)
            .context("provider revision overflow")?;
        if running {
            session.provider_state(false).await?;
        }
        session.track_local("Publish standing paid Serve settings".into(), || {
            self.publish(expected, next)
        })?;
        if running {
            session.provider_state(true).await?;
        }
        self.view()
    }
    pub(crate) fn used(&self, reference: &str, fingerprint: &str) -> anyhow::Result<()> {
        let now = hellas_auth::authority::UnixMillis::now()?.0;
        let mut held = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("provider configuration lock failed"))?;
        let mut next = held.clone();
        let mut changed = false;
        for key in next
            .endpoints
            .values_mut()
            .flat_map(|e| e.keys.values_mut())
            .filter(|key| key.reference == reference && key.fingerprint == fingerprint)
        {
            key.last_use = key.last_use.max(Some(now));
            changed = true;
        }
        if changed {
            document::write(self, &next, &held)?;
            *held = next;
        }
        Ok(())
    }
    fn metadata(config: &Configuration) -> ProviderMetadata {
        ProviderMetadata {
            revision: config.revision,
            endpoints: config
                .endpoints
                .values()
                .map(|e| EndpointMetadata {
                    service: e.service.clone(),
                    method: e.method.clone(),
                    allowed_hosts: e.hosts.clone(),
                    keys: e.keys.keys().cloned().collect(),
                })
                .collect(),
        }
    }
    fn http(&self, endpoint: &Endpoint) -> anyhow::Result<HttpFetchProvider> {
        let credentials = endpoint
            .keys
            .iter()
            .map(|(name, key)| {
                (
                    name.clone(),
                    HttpCredential {
                        allowed_origins: key.origins.clone(),
                        allowed_paths: key.paths.clone(),
                        allowed_methods: key.methods.clone(),
                        trust_roots: hellas_rpc::http_fetch::HttpTrustRoots::WebPki,
                        header_name: key.header.clone(),
                        header_value: self.vault.source(&key.reference, &key.prefix),
                    },
                )
            })
            .collect();
        Ok(HttpFetchProvider::new(
            HttpEgressPolicy {
                allowed_hosts: endpoint.hosts.clone(),
                allow_private_addresses: endpoint.private,
            },
            credentials,
        )?)
    }
    fn validate(&self, config: &Configuration) -> anyhow::Result<Vec<ResourceFile>> {
        ensure!(
            config.endpoints.len() <= 16,
            "at most 16 endpoints are supported"
        );
        for endpoint in config.endpoints.values() {
            self.http(endpoint)?;
            let mut resources = config.resources.iter().filter(|resource| {
                resource.service == endpoint.service && resource.method == endpoint.method
            });
            if let Some(first) = resources.next() {
                for resource in resources {
                    match (&first.resource, &resource.resource) {
                        (ResourceKind::Https { .. }, ResourceKind::Https { .. }) => {}
                        (
                            ResourceKind::OpenAi {
                                credential: first, ..
                            },
                            ResourceKind::OpenAi { credential, .. },
                        ) => {
                            ensure!(
                                first == credential,
                                "all resources on one OpenAI endpoint must use the same credential alias"
                            );
                        }
                        _ => anyhow::bail!("one endpoint cannot mix resource drivers"),
                    }
                }
            }
        }
        if let Some(input) = &config.paid {
            crate::paid::serve_config(&self.directory, input)?;
            let resource = config
                .resources
                .iter()
                .find(|r| r.service == input.policy.service && r.method == input.policy.method)
                .context("paid work requires a configured Fetch resource on its selected route")?;
            ensure!(
                matches!(
                    (&resource.resource, input.policy.presentation),
                    (ResourceKind::OpenAi { .. }, FetchPresentation::Responses)
                        | (ResourceKind::Https { .. }, FetchPresentation::Http)
                ),
                "paid presentation differs from the route implementation"
            );
        }
        let canonical = document::read(&self.config_path)?;
        let previous = document::view(&canonical, &self.directory)?;
        let mut names = std::collections::BTreeSet::new();
        let mut policies = Vec::new();
        for resource in &config.resources {
            ensure!(names.insert(&resource.name), "duplicate resource name");
            let route = route_id(&resource.service, &resource.method);
            let endpoint = config
                .endpoints
                .get(&route)
                .context("resource endpoint is missing")?;
            let account = id("hellas.gate.account.v1", &route);
            let resource_id = id("hellas.gate.resource.v1", &resource.name);
            let revision = Revision(*config.policy_revisions.get(&resource.name).unwrap_or(&1));
            let pricing = Pricing {
                revision,
                rates: None,
            };
            if let ResourceKind::OpenAi { credential, .. } = &resource.resource {
                ensure!(
                    endpoint.hosts == ["api.openai.com"] && !endpoint.private,
                    "OpenAI requires its explicit public endpoint"
                );
                let key = endpoint
                    .keys
                    .get(credential)
                    .context("select an upstream key")?;
                ensure!(
                    key.origins == ["https://api.openai.com"]
                        && key.paths == ["/v1/responses"]
                        && key.methods == ["POST"]
                        && key.header.eq_ignore_ascii_case("authorization")
                        && key.prefix == "Bearer ",
                    "OpenAI key must be scoped to POST /v1/responses"
                );
            }
            let existing = canonical
                .grants
                .as_ref()
                .and_then(|g| g.resources.iter().find(|r| r.name == resource.name));
            if let Some(existing) = existing
                && previous.resources.iter().any(|r| r == resource)
                && existing.revision == revision
            {
                document::policy(existing).validate()?;
                continue;
            }
            let responses = matches!(resource.resource, ResourceKind::OpenAi { .. });
            let mut policy = match existing.filter(|p| {
                if responses {
                    p.responses.is_some()
                } else {
                    p.https.is_some()
                }
            }) {
                Some(existing) => existing.clone(),
                None => ResourceFile {
                    resource: existing.map_or(resource_id, |p| p.resource),
                    account: existing.map_or(account, |p| p.account),
                    revision,
                    name: resource.name.clone(),
                    invocation_policy: InvocationPolicy {
                        admissions: vec![Admission::Fetch {
                            admission: FetchAdmission {
                                route: WorkRoute::sealed_route(
                                    &resource.service,
                                    &resource.method,
                                )?,
                                contract: if responses {
                                    hellas_providers::responses_contract()?
                                } else {
                                    hellas_rpc::FetchContract::http()?
                                },
                                limits: FetchLimits {
                                    max_request_body_bytes: 64 << 10,
                                    max_output_events: 4096,
                                    max_spool_bytes: if responses { 4 << 20 } else { 8 << 20 },
                                    max_encoded_result_frame: 4 << 20,
                                    max_encoded_prepared_input: 128 << 10,
                                },
                            },
                        }],
                        limits: Limits {
                            max_upload_bytes: 128 << 10,
                            max_output_bytes: if responses { 1 << 20 } else { 4 << 20 },
                            max_steps: 1,
                            execution_timeout_ms: 300_000,
                        },
                    },
                    https: None,
                    responses: None,
                    pricing,
                },
            };
            // Apply only represented fields to the canonical resource, including
            // new resources. Unrepresented policy and limits never round-trip.
            match &resource.resource {
                ResourceKind::OpenAi {
                    model,
                    max_output_tokens,
                    ..
                } => {
                    let responses = policy.responses.get_or_insert(ResponsesResource {
                        model: String::new(),
                        max_output_tokens: 0,
                        settings: Default::default(),
                    });
                    responses.model = model.clone();
                    responses.max_output_tokens = *max_output_tokens;
                }
                ResourceKind::Https {
                    origin,
                    paths,
                    methods,
                    credential,
                    max_response_bytes,
                    tls_roots,
                } => {
                    if let Some(key) = credential {
                        ensure!(
                            endpoint.keys.contains_key(key),
                            "unknown HTTPS credential alias"
                        );
                    }
                    let https = policy.https.get_or_insert(HttpsResource {
                        origin: String::new(),
                        paths: vec![],
                        methods: vec![],
                        credential: None,
                        tls: hellas_rpc::http_fetch::HttpTls {
                            roots: hellas_rpc::http_fetch::HttpTrustRoots::WebPki,
                            spki_sha256: vec![],
                        },
                        accounting: hellas_authority::model::http_usage::AccountingProfile::None,
                        max_output_tokens: 0,
                        max_response_bytes: 0,
                        model_policy: None,
                    });
                    https.origin = origin.clone();
                    https.paths = paths.clone();
                    https.methods = methods.clone();
                    https.credential = credential.clone();
                    https.max_response_bytes = *max_response_bytes;
                    https.tls.roots = if tls_roots.is_empty() {
                        hellas_rpc::http_fetch::HttpTrustRoots::WebPki
                    } else {
                        hellas_rpc::http_fetch::HttpTrustRoots::Certificates {
                            der_base64: tls_roots.clone(),
                        }
                    };
                }
            }
            policy.revision = revision;
            document::policy(&policy).validate()?;
            policies.push(policy);
        }
        Ok(policies)
    }
    pub fn factory(&self, local: Arc<LocalNode>) -> ProviderFactory {
        let composition = self.clone();
        let prepared = self.clone();
        ProviderFactory {
            prepare: Some(Arc::new(move |update| {
                Ok(Box::new(prepared.prepare(update)?) as Box<dyn PreparedProviderConfiguration>)
            })),
            start: Arc::new(move |control| {
                let (composition, local) = (composition.clone(), local.clone());
                Box::pin(async move {
                    Ok(composition
                        .start(local, control.context("host authority is required")?)
                        .await?)
                })
            }),
        }
    }
    fn prepare(&self, update: ProviderUpdate) -> anyhow::Result<Staged> {
        let old = self.snapshot()?;
        ensure!(
            update.expected_revision == old.revision,
            "provider revision conflict"
        );
        let mut next = old.clone();
        next.revision = next
            .revision
            .checked_add(1)
            .context("provider revision overflow")?;
        let mut secret = None;
        let source_key = Self::reference_key(&update);
        let replacing = matches!(&update.change, Some(UpstreamChange::ReplaceKey(_)));
        match update.change.context("provider change is required")? {
            UpstreamChange::SetEndpoint(e) => {
                let route = route_id(&e.service, &e.method);
                FetchRouteEntry::new(
                    Arc::new(HttpFetchProvider::new(
                        HttpEgressPolicy {
                            allowed_hosts: e.allowed_hosts.clone(),
                            allow_private_addresses: e.allow_private_addresses,
                        },
                        BTreeMap::new(),
                    )?),
                    Arc::new(HttpFetchAdaptorFactory::new()?),
                    FetchRoutePolicy::default(),
                )?;
                ensure!(
                    !e.service.is_empty()
                        && !e.method.is_empty()
                        && !e.service.contains('/')
                        && !e.method.contains('/'),
                    "invalid route name"
                );
                let keys = next
                    .endpoints
                    .remove(&route)
                    .map(|e| e.keys)
                    .unwrap_or_default();
                next.endpoints.insert(
                    route,
                    Endpoint {
                        service: e.service,
                        method: e.method,
                        hosts: e.allowed_hosts,
                        private: e.allow_private_addresses,
                        keys,
                    },
                );
            }
            UpstreamChange::RemoveEndpoint(e) => {
                ensure!(
                    next.endpoints
                        .remove(&route_id(&e.service, &e.method))
                        .is_some(),
                    "endpoint does not exist"
                );
                next.resources
                    .retain(|r| r.service != e.service || r.method != e.method);
            }
            UpstreamChange::AddKey(k) | UpstreamChange::ReplaceKey(k) => {
                let endpoint = next
                    .endpoints
                    .get_mut(&route_id(&k.service, &k.method))
                    .context("endpoint does not exist")?;
                ensure!(
                    endpoint.keys.contains_key(&k.key) == replacing,
                    "key exists or replacement key is missing"
                );
                ensure!(
                    !k.key.is_empty()
                        && k.key.len() <= 64
                        && !k.secret.is_empty()
                        && k.secret.len() <= 2048,
                    "invalid upstream key"
                );
                hellas_rpc::http_fetch::check_headers(
                    &[(k.header_name.clone(), format!("{}{}", k.prefix, k.secret))],
                    true,
                )?;
                let declared = if let Some(key) = &source_key {
                    self.sources
                        .lock()
                        .map_err(|_| anyhow::anyhow!("configuration source lock failed"))?
                        .remove(key)
                } else {
                    None
                };
                if let Some(reference) = &declared {
                    let value = self
                        .vault
                        .blocking_get(reference)?
                        .context("declared secret source disappeared")?;
                    ensure!(
                        value.as_slice() == k.secret.as_bytes(),
                        "declared secret source changed; preview again"
                    );
                }
                let reference = match &declared {
                    Some(reference) => reference.clone(),
                    None => format!("upstream-{}", hex::encode(crate::identity::random::<16>()?)),
                };
                endpoint.keys.insert(
                    k.key,
                    Key {
                        origins: k.allowed_origins,
                        paths: k.allowed_paths,
                        methods: k.allowed_methods,
                        header: k.header_name,
                        prefix: k.prefix,
                        reference: reference.clone(),
                        last_use: None,
                        fingerprint: hex::encode(
                            &Digest::hash(k.secret.as_bytes()).as_bytes()[..6],
                        ),
                    },
                );
                if declared.is_none() {
                    secret = Some((reference, zeroize::Zeroizing::new(k.secret.into_bytes())));
                }
            }
            UpstreamChange::RemoveKey(k) => {
                ensure!(
                    next.endpoints
                        .get_mut(&route_id(&k.service, &k.method))
                        .context("endpoint does not exist")?
                        .keys
                        .remove(&k.key)
                        .is_some(),
                    "key does not exist"
                );
            }
        }
        self.validate(&next)?;
        if let Some((key, value)) = &secret {
            self.vault.blocking_set(key, value)?;
        }
        Ok(Staged {
            composition: self.clone(),
            old,
            next,
            secret,
            committed: false,
        })
    }
    async fn start(
        &self,
        local: Arc<LocalNode>,
        control: NodeControl,
    ) -> anyhow::Result<hellas_sdk::ProviderHandle> {
        let config = self.snapshot()?;
        let doc = document::read(&self.config_path)?;
        let grants = doc.grants(Some(&self.directory))?;
        let bound = local
            .coordinator
            .provider()
            .await?
            .context("bind a provider first")?;
        let identity = ProviderIdentity::load_or_create(
            &self.directory,
            &local,
            matches!(bound.platform, hellas_rpc::PlatformEnrollment::Absent),
        )
        .await?;
        ensure!(
            identity.enrollment == bound,
            "bound provider differs from factory identity"
        );
        let accounts = hellas_providers::account::AccountRegistry::new(&doc.upstream_accounts)?;
        let mut routes = FetchRouteRegistry::new();
        for route in &doc.routes {
            let entry = route
                .destination
                .clone()
                .into_entry(route.capabilities.clone().into_policy()?)?;
            #[cfg(test)]
            let entry = match (
                &route.destination,
                self.responses_origin
                    .lock()
                    .map_err(|_| anyhow::anyhow!("response fixture lock failed"))?
                    .as_ref(),
            ) {
                (hellas_sdk::serve_config::Destination::OpenAiResponses { .. }, Some(origin)) => {
                    FetchRouteEntry::new(
                        crate::responses_fixture::provider(origin),
                        Arc::new(hellas_providers::OpenAiResponsesAdaptorFactory::new()?),
                        route.capabilities.clone().into_policy()?,
                    )?
                }
                _ => entry,
            };
            let accounting = match &route.destination {
                hellas_sdk::serve_config::Destination::OpenAiResponses {
                    credential: credential_name,
                    ..
                } => config
                    .endpoints
                    .get(&route_id(&route.service, &route.method))
                    .and_then(|e| e.keys.get(credential_name))
                    .map(|k| crate::usage::Key::Fixed(k.reference.clone(), k.fingerprint.clone())),
                hellas_sdk::serve_config::Destination::Http { .. } => config
                    .endpoints
                    .get(&route_id(&route.service, &route.method))
                    .map(|e| {
                        crate::usage::Key::Http(
                            e.keys
                                .iter()
                                .map(|(alias, k)| {
                                    (alias.clone(), (k.reference.clone(), k.fingerprint.clone()))
                                })
                                .collect(),
                        )
                    }),
                _ => None,
            };
            let entry = if let Some(key) = accounting {
                entry.map_provider(|driver| crate::usage::track(driver, self.clone(), key))?
            } else {
                entry
            };
            let entry = route.bind_accounts(entry, &accounts)?;
            routes.register(FetchRoute::new(&route.service, &route.method), entry)?;
        }
        let options = hellas_sdk::FetchProviderOptions {
            port: None,
            identity: local.identity.node.clone(),
            enrollment: identity.enrollment,
            root: identity.root,
            state_directory: self.directory.join("provider"),
            grants: grants.map(|g| hellas_sdk::GrantProviderOptions {
                configuration: g.configuration,
                max_job_millis: g.max_job_millis,
            }),
            routes,
            fetch_max_in_flight: hellas_rpc::DEFAULT_FETCH_MAX_IN_FLIGHT,
            fetch_queue_capacity: hellas_rpc::DEFAULT_FETCH_QUEUE_CAPACITY,
            paid_work: if let Some(terms) = &doc.paid {
                let policy = local
                    .coordinator
                    .paid_policy()
                    .await?
                    .context("approve the standing paid policy before starting paid Serve")?;
                ensure!(
                    policy.policy == terms.policy()?,
                    "standing paid settings differ from the owner-approved policy"
                );
                let paid = match &terms.chain {
                    Some(c) => c.clone().into_config()?,
                    None => hellas_sdk::client_chain::devnet(&self.directory.join("provider"))?,
                };
                let node = local.chain.get(&paid).await?;
                Some(hellas_sdk::PaidProviderOptions::new(paid, node)?)
            } else {
                None
            },
        }
        .prepare()
        .await?
        .on_host(control)?;
        Ok(hellas_sdk::start_fetch_provider(options)
            .await?
            .with_configuration(Self::metadata(&config)))
    }
}
struct Staged {
    composition: Composition,
    old: Configuration,
    next: Configuration,
    secret: Option<(String, zeroize::Zeroizing<Vec<u8>>)>,
    committed: bool,
}
impl PreparedProviderConfiguration for Staged {
    fn commit(&mut self) -> Result<ProviderMetadata, ProviderErrorBox> {
        self.composition
            .publish(self.old.revision, self.next.clone())?;
        self.committed = true;
        Ok(Composition::metadata(&self.next))
    }
    fn rollback(&mut self) -> Result<(), ProviderErrorBox> {
        if self.committed {
            self.composition
                .publish(self.next.revision, self.old.clone())?;
        }
        self.committed = false;
        Ok(())
    }
}

pub fn update(expected_revision: u64, change: ProviderChange) -> ProviderUpdate {
    let key = |k: UpstreamKeyInput| UpstreamKey {
        service: k.service,
        method: k.method,
        key: k.key,
        allowed_origins: k.origins,
        allowed_paths: k.paths,
        allowed_methods: k.methods,
        header_name: k.header,
        prefix: k.prefix,
        secret: k.secret,
    };
    ProviderUpdate {
        expected_revision,
        change: Some(match change {
            ProviderChange::Endpoint {
                service,
                method,
                hosts,
                allow_private_addresses,
            } => UpstreamChange::SetEndpoint(UpstreamEndpoint {
                service,
                method,
                allowed_hosts: hosts,
                allow_private_addresses,
            }),
            ProviderChange::RemoveEndpoint { service, method } => {
                UpstreamChange::RemoveEndpoint(UpstreamRoute { service, method })
            }
            ProviderChange::AddKey { key: k } => UpstreamChange::AddKey(key(k)),
            ProviderChange::ReplaceKey { key: k } => UpstreamChange::ReplaceKey(key(k)),
            ProviderChange::RemoveKey {
                service,
                method,
                key,
            } => UpstreamChange::RemoveKey(UpstreamKeyId {
                service,
                method,
                key,
            }),
        }),
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        let mut obsolete = std::collections::BTreeSet::new();
        if self.committed {
            let retained: std::collections::BTreeSet<_> = self
                .next
                .endpoints
                .values()
                .flat_map(|e| e.keys.values().map(|k| k.reference.as_str()))
                .collect();
            for key in self.old.endpoints.values().flat_map(|e| e.keys.values()) {
                if !retained.contains(key.reference.as_str()) {
                    obsolete.insert(key.reference.clone());
                }
            }
        } else if let Some((reference, _)) = &self.secret {
            obsolete.insert(reference.clone());
        }
        for reference in obsolete {
            if self.composition.vault.blocking_delete(&reference).is_err() {
                tracing::warn!("retired upstream vault entry could not be removed");
            }
        }
    }
}

pub async fn configure_resource(
    local: &LocalNode,
    session: &crate::control::Session,
    expected: u64,
    input: ResourceInput,
) -> anyhow::Result<ProviderView> {
    configure_resources(local, session, expected, vec![input]).await
}
pub async fn configure_resources(
    local: &LocalNode,
    session: &crate::control::Session,
    expected: u64,
    inputs: Vec<ResourceInput>,
) -> anyhow::Result<ProviderView> {
    use hellas_rpc::pb::host::GetHostStatusRequest;
    anyhow::ensure!(
        local.composition.view()?.revision == expected,
        "provider revision conflict"
    );
    let client = &session.client;
    let status = client
        .get_host_status(GetHostStatusRequest {
            authorization: local.authorization(),
        })
        .await?;
    anyhow::ensure!(
        status.provider.as_ref().is_some_and(|s| s.state == 2),
        "start the provider before administering resources"
    );
    let active = match session
        .work(hellas_authority::model::grant::admin::WorkControlCommand::Catalogue)
        .await?
    {
        hellas_authority::model::grant::admin::WorkControlReply::Catalogue(policies) => policies,
        _ => anyhow::bail!("invalid authority resource catalogue"),
    };
    let names = inputs
        .iter()
        .map(|input| input.name.clone())
        .collect::<Vec<_>>();
    session.track_local(
        format!("Publish resource catalogue {}", names.join(", ")),
        || local.composition.catalogue_many(expected, inputs, &active),
    )?;
    session.provider_state(false).await?;
    session
        .provider_state(true)
        .await
        .context("restarting provider with the prepared resource catalogue")?;
    local.composition.view()
}

pub struct ProviderStep {
    pub expected: u64,
    pub after_resources: bool,
    pub change: ProviderChange,
    pub reference: Option<String>,
}
impl Composition {
    fn reference_key(update: &ProviderUpdate) -> Option<String> {
        let key = match update.change.as_ref()? {
            UpstreamChange::AddKey(k) | UpstreamChange::ReplaceKey(k) => k,
            _ => return None,
        };
        Some(hex::encode(
            hellas_auth::hash_tuple(
                "hellas.gate.configuration-source.v1",
                &[
                    &update.expected_revision.to_be_bytes(),
                    key.service.as_bytes(),
                    key.method.as_bytes(),
                    key.key.as_bytes(),
                    Digest::hash(key.secret.as_bytes()).as_bytes(),
                ],
            )
            .as_bytes(),
        ))
    }
    pub async fn apply(
        &self,
        session: &crate::control::Session,
        expected: u64,
        mut change: ProviderChange,
        reference: Option<String>,
    ) -> anyhow::Result<()> {
        if let Some(reference) = &reference {
            let bytes = self.vault.required(reference).await?;
            let value = std::str::from_utf8(&bytes)?.to_owned();
            match &mut change {
                ProviderChange::AddKey { key } | ProviderChange::ReplaceKey { key } => {
                    key.secret = value
                }
                _ => anyhow::bail!("secret reference requires a key change"),
            }
        }
        let update = update(expected, change);
        let key = Self::reference_key(&update);
        if let Some(reference) = reference {
            let key = key
                .clone()
                .context("a secret reference requires a key operation")?;
            self.sources
                .lock()
                .map_err(|_| anyhow::anyhow!("configuration source lock failed"))?
                .insert(key, reference);
        }
        let result = session
            .track(
                format!("Configure provider at revision {expected}"),
                false,
                session
                    .client
                    .configure_provider(hellas_rpc::pb::host::ConfigureProviderRequest {
                        authorization: session.authorization.clone(),
                        update: Some(update),
                    }),
            )
            .await;
        if let Some(key) = key {
            self.sources
                .lock()
                .map_err(|_| anyhow::anyhow!("configuration source lock failed"))?
                .remove(&key);
        }
        result?;
        Ok(())
    }
    pub async fn diff(
        &self,
        upstreams: &[DesiredUpstream],
        resources: &[ResourceInput],
        paid: Option<&Option<PaidServeInput>>,
    ) -> anyhow::Result<Vec<ProviderStep>> {
        ensure!(
            upstreams.len() <= 16 && resources.len() <= 16,
            "at most 16 upstreams/resources are supported"
        );
        let old = self.snapshot()?;
        let mut desired = old.clone();
        desired.endpoints.clear();
        desired.resources = resources.to_vec();
        if let Some(paid) = paid {
            desired.paid = paid.clone();
        }
        let mut changes = Vec::new();
        let mut revision = old.revision;
        for upstream in upstreams {
            let route = route_id(&upstream.service, &upstream.method);
            ensure!(
                !desired.endpoints.contains_key(&route),
                "duplicate upstream"
            );
            ensure!(upstream.keys.len() <= 32, "at most 32 keys per upstream");
            let current = old.endpoints.get(&route);
            let mut expanded_hosts = current
                .map(|endpoint| endpoint.hosts.clone())
                .unwrap_or_default();
            for host in &upstream.hosts {
                if !expanded_hosts.contains(host) {
                    expanded_hosts.push(host.clone());
                }
            }
            let expanded_private = upstream.allow_private_addresses
                || current.is_some_and(|endpoint| endpoint.private);
            if current.is_none_or(|endpoint| {
                endpoint.hosts != expanded_hosts || endpoint.private != expanded_private
            }) {
                changes.push(ProviderStep {
                    expected: revision,
                    after_resources: false,
                    change: ProviderChange::Endpoint {
                        service: upstream.service.clone(),
                        method: upstream.method.clone(),
                        hosts: expanded_hosts.clone(),
                        allow_private_addresses: expanded_private,
                    },
                    reference: None,
                });
                revision = revision
                    .checked_add(1)
                    .context("provider revision overflow")?;
            }
            if expanded_hosts != upstream.hosts
                || expanded_private != upstream.allow_private_addresses
            {
                changes.push(ProviderStep {
                    expected: revision,
                    after_resources: true,
                    change: ProviderChange::Endpoint {
                        service: upstream.service.clone(),
                        method: upstream.method.clone(),
                        hosts: upstream.hosts.clone(),
                        allow_private_addresses: upstream.allow_private_addresses,
                    },
                    reference: None,
                });
                revision = revision
                    .checked_add(1)
                    .context("provider revision overflow")?;
            }
            let mut keys = BTreeMap::new();
            for key in &upstream.keys {
                ensure!(!keys.contains_key(&key.name), "duplicate key label");
                let value = self
                    .vault
                    .required(&key.secret_reference)
                    .await
                    .context("declared key source is unavailable")?;
                let value = std::str::from_utf8(&value)?;
                ensure!(
                    !value.is_empty() && value.len() <= 2048,
                    "invalid upstream secret source"
                );
                hellas_rpc::http_fetch::check_headers(
                    &[(key.header.clone(), format!("{}{value}", key.prefix))],
                    true,
                )?;
                let new = Key {
                    origins: key.origins.clone(),
                    paths: key.paths.clone(),
                    methods: key.methods.clone(),
                    header: key.header.clone(),
                    prefix: key.prefix.clone(),
                    reference: key.secret_reference.clone(),
                    fingerprint: hex::encode(&Digest::hash(value.as_bytes()).as_bytes()[..6]),
                    last_use: None,
                };
                let previous = current.and_then(|e| e.keys.get(&key.name));
                let same = previous.is_some_and(|k| {
                    k.origins == new.origins
                        && k.paths == new.paths
                        && k.methods == new.methods
                        && k.header == new.header
                        && k.prefix == new.prefix
                        && k.reference == new.reference
                        && k.fingerprint == new.fingerprint
                });
                if !same {
                    let input = UpstreamKeyInput {
                        service: upstream.service.clone(),
                        method: upstream.method.clone(),
                        key: key.name.clone(),
                        origins: key.origins.clone(),
                        paths: key.paths.clone(),
                        methods: key.methods.clone(),
                        header: key.header.clone(),
                        prefix: key.prefix.clone(),
                        secret: String::new(),
                    };
                    let change = if previous.is_some() {
                        ProviderChange::ReplaceKey { key: input }
                    } else {
                        ProviderChange::AddKey { key: input }
                    };
                    changes.push(ProviderStep {
                        expected: revision,
                        after_resources: false,
                        change,
                        reference: Some(key.secret_reference.clone()),
                    });
                    revision = revision
                        .checked_add(1)
                        .context("provider revision overflow")?;
                }
                keys.insert(key.name.clone(), new);
            }
            if let Some(current) = current {
                for name in current.keys.keys().filter(|name| !keys.contains_key(*name)) {
                    changes.push(ProviderStep {
                        expected: revision,
                        after_resources: true,
                        change: ProviderChange::RemoveKey {
                            service: upstream.service.clone(),
                            method: upstream.method.clone(),
                            key: name.clone(),
                        },
                        reference: None,
                    });
                    revision = revision
                        .checked_add(1)
                        .context("provider revision overflow")?;
                }
            }
            desired.endpoints.insert(
                route,
                Endpoint {
                    service: upstream.service.clone(),
                    method: upstream.method.clone(),
                    hosts: upstream.hosts.clone(),
                    private: upstream.allow_private_addresses,
                    keys,
                },
            );
        }
        for endpoint in old.endpoints.values().filter(|e| {
            !desired
                .endpoints
                .contains_key(&route_id(&e.service, &e.method))
        }) {
            changes.push(ProviderStep {
                expected: revision,
                after_resources: true,
                change: ProviderChange::RemoveEndpoint {
                    service: endpoint.service.clone(),
                    method: endpoint.method.clone(),
                },
                reference: None,
            });
            revision = revision
                .checked_add(1)
                .context("provider revision overflow")?;
        }
        for input in resources {
            if let Some(previous) = old.resources.iter().find(|r| r.name == input.name) {
                ensure!(
                    previous.service == input.service && previous.method == input.method,
                    "a resource cannot move to another upstream account"
                );
            }
            let previous = *old.policy_revisions.get(&input.name).unwrap_or(&0);
            let changed = !old.resources.contains(input);
            desired.policy_revisions.insert(
                input.name.clone(),
                if changed {
                    previous
                        .checked_add(1)
                        .context("resource revision overflow")?
                } else {
                    previous
                },
            );
        }
        self.validate(&desired)?;
        Ok(changes)
    }
    pub fn retire_catalogue(&self, expected: u64, name: &str) -> anyhow::Result<()> {
        let mut config = self.snapshot()?;
        ensure!(config.revision == expected, "provider revision conflict");
        if !config.resources.iter().any(|r| r.name == name) {
            return Ok(());
        }
        config.resources.retain(|r| r.name != name);
        config.revision = config
            .revision
            .checked_add(1)
            .context("provider revision overflow")?;
        self.publish(expected, config)
    }
}

#[cfg(test)]
pub(crate) fn declare_test_account(
    directory: &Path,
    service: &str,
    method: &str,
    aliases: &[&str],
    quota_url: &str,
) {
    let path = directory.join("serve.json");
    let mut doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    doc["upstream_accounts"] = serde_json::json!({"fixture":{
        "ledger":directory.join("account-budget.sqlite"),"window_seconds":86400,"max_requests":10000,
        "max_tokens":1000000,"max_concurrency":8,"max_request_bytes":65536,"max_output_tokens":256,
        "accounting":"openai-responses","quota":{"url":quota_url,"provider":"fixture",
            "upstream":"openai","account":"personal","max_age_seconds":600,
            "reserve":{"type":"remaining-ratio","minimum":0.2,"scope":"all","windows":["seven_day"]}}
    }});
    let route = doc["routes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["service"] == service && r["method"] == method)
        .unwrap();
    route["credential_accounts"] = serde_json::to_value(
        aliases
            .iter()
            .map(|alias| (*alias, "fixture"))
            .collect::<BTreeMap<_, _>>(),
    )
    .unwrap();
    route["capabilities"] = serde_json::json!({"max_output_tokens":256});
    vault::write(&path, &doc).unwrap();
    hellas_sdk::serve_config::ServeConfig::load(&path).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::work;
    use crate::state::AppState;
    use hellas_authority::model::grant::admin::{WorkControlCommand, WorkControlReply};
    use hellas_rpc::pb::host::*;
    #[test]
    fn canonical_resource_edit_preserves_security_fields_and_untouched_limits() {
        use hellas_authority::model::grant::resource::{AccountingProfile, PathModelPolicy};
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let path = directory.path().join("serve.json");
        vault::write(&path, &serde_json::json!({"version":1,"routes":[{
            "service":"api","method":"request","destination":{"type":"http","config":{"allowed_hosts":["example.com"]}}
        }]})).unwrap();
        let gate = Composition::open(directory.path(), Vault::fixture(secrets.path())).unwrap();
        let input = ResourceInput {
            name: "locked".into(),
            service: "api".into(),
            method: "request".into(),
            resource: ResourceKind::Https {
                origin: "https://example.com".into(),
                paths: vec!["/chat".into()],
                methods: vec!["POST".into()],
                credential: None,
                max_response_bytes: 4096,
                tls_roots: vec![],
            },
        };
        gate.catalogue_many(0, vec![input], &[]).unwrap();
        let mut doc = document::read(&path).unwrap();
        let resource = &mut doc.grants.as_mut().unwrap().resources[0];
        resource.resource = ResourceId(Digest::hash(b"imported resource"));
        resource.account = ResourceId(Digest::hash(b"imported account"));
        resource.revision = Revision(17);
        resource.pricing.revision = Revision(17);
        let imported_ids = (resource.resource, resource.account);
        let https = resource.https.as_mut().unwrap();
        https.tls.spki_sha256 = vec!["ab".repeat(32)];
        https.accounting = AccountingProfile::OpenaiChat;
        https.max_output_tokens = 16;
        https.model_policy = Some(vec![PathModelPolicy {
            path: "/chat".into(),
            models: vec!["reviewed-model".into()],
        }]);
        let mut expected = https.clone();
        expected.max_response_bytes = 8192;
        resource.invocation_policy.limits.execution_timeout_ms = 120000;
        resource.invocation_policy.limits.max_output_bytes = 65536;
        let expected_policy = resource.invocation_policy.clone();
        doc.grants.as_mut().unwrap().accounts[0].account = imported_ids.1;
        doc.routes[0].capabilities.models = vec!["reviewed-model".into()];
        doc.routes[0].capabilities.max_output_tokens = Some(16);
        vault::write(&path, &doc).unwrap();
        let mut edited = gate.snapshot().unwrap().resources.remove(0);
        let ResourceKind::Https {
            max_response_bytes, ..
        } = &mut edited.resource
        else {
            panic!("HTTPS")
        };
        *max_response_bytes = 8192;
        gate.catalogue_many(1, vec![edited], &[gate.policy("locked").unwrap()])
            .unwrap();
        let saved = document::read(&path).unwrap();
        let resource = &saved.grants.as_ref().unwrap().resources[0];
        assert_eq!(resource.https.as_ref(), Some(&expected));
        assert_eq!((resource.resource, resource.account), imported_ids);
        assert_eq!(resource.revision, Revision(18));
        assert_eq!(resource.pricing.revision, Revision(17));
        assert_eq!(resource.invocation_policy, expected_policy);
        assert_eq!(
            saved.routes[0].capabilities.models,
            doc.routes[0].capabilities.models
        );
        assert_eq!(saved.routes[0].capabilities.max_output_tokens, Some(16));
    }
    #[test]
    fn metadata_edit_preserves_a_sealed_destination_without_grant_resources() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let path = directory.path().join("serve.json");
        vault::write(
            &path,
            &serde_json::json!({"version":1,"routes":[{
                "service":"openai","method":"responses","destination":{"type":"http","config":{}}
            }]}),
        )
        .unwrap();
        declare_test_account(
            directory.path(),
            "openai",
            "responses",
            &["personal"],
            "http://127.0.0.1:9/metrics",
        );
        let mut doc = document::read(&path).unwrap();
        doc.routes[0].destination = serde_json::from_value(serde_json::json!({"type":"open-ai-responses",
            "credential":"personal","credentials":{"personal":{"secret_env":"GATE_TEST_UNUSED_TOKEN","prefix":"sk-"}}})).unwrap();
        vault::write(&path, &doc).unwrap();
        let gate = Composition::open(directory.path(), Vault::fixture(secrets.path())).unwrap();
        let mut view = gate.snapshot().unwrap();
        view.endpoints
            .get_mut("openai/responses")
            .unwrap()
            .keys
            .get_mut("personal")
            .unwrap()
            .last_use = Some(42);
        view.revision += 1;
        gate.publish(0, view).unwrap();
        let saved = document::read(&path).unwrap();
        assert_eq!(
            serde_json::to_value(&saved.routes[0].destination).unwrap(),
            serde_json::to_value(&doc.routes[0].destination).unwrap()
        );
        assert_eq!(
            saved.routes[0].credential_metadata["personal"].last_use,
            Some(42)
        );
    }
    #[test]
    fn canonical_cli_config_drives_gate_and_external_edits_are_reloaded() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let path = directory.path().join("serve.json");
        let bytes = br#"{
            "version":1,"revision":23,"port":18444,
            "routes":[{"service":"shared","method":"request","destination":{"type":"http","config":{}}}]
        }"#;
        // This is the exact SDK loader called by hellas serve --config.
        let cli = hellas_sdk::serve_config::ServeConfig::from_bytes(bytes).unwrap();
        vault::write(&path, &cli).unwrap();
        let gate = Composition::open(directory.path(), Vault::fixture(secrets.path())).unwrap();
        assert_eq!(gate.listen_port().unwrap(), Some(18444));
        assert_eq!(gate.snapshot().unwrap().revision, 23);
        assert_eq!(gate.snapshot().unwrap().endpoints.len(), 1);
        // A CLI/operator edit to the shared document is visible without a UI
        // composition export, second loader or application restart.
        let mut edited = hellas_sdk::serve_config::ServeConfig::load(&path).unwrap();
        edited.revision = 24;
        edited.port = Some(18445);
        vault::write(&path, &edited).unwrap();
        assert_eq!(gate.listen_port().unwrap(), Some(18445));
        assert_eq!(gate.snapshot().unwrap().revision, 24);
    }
    #[test]
    fn editing_shared_config_preserves_explicit_credential_trust_roots() {
        use base64::Engine as _;
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let roots = hellas_rpc::http_fetch::HttpTrustRoots::Certificates {
            der_base64: vec![
                base64::engine::general_purpose::STANDARD
                    .encode(hellas_attestation::apple_app_attest_root_ca()),
            ],
        };
        // Public certificate bytes only; the environment credential is a
        // reference and is never resolved by this configuration edit.
        let value = serde_json::json!({"version":1,"revision":0,"routes":[{
            "service":"private","method":"request","destination":{"type":"http","config":{
                "credentials":{"account":{
                    "allowed_origins":["https://example.com"],"allowed_paths":["/"],
                    "allowed_methods":["POST"],"header_name":"authorization",
                    "trust_roots":roots,"secret_env":"GATE_TEST_UNUSED_TOKEN","prefix":"Bearer "
                }}
            }}
        }]});
        vault::write(&directory.path().join("serve.json"), &value).unwrap();
        declare_test_account(
            directory.path(),
            "private",
            "request",
            &["account"],
            "http://127.0.0.1:9/metrics",
        );
        let gate = Composition::open(directory.path(), Vault::fixture(secrets.path())).unwrap();
        let mut view = gate.snapshot().unwrap();
        view.revision = 1;
        gate.publish(0, view).unwrap();
        let saved =
            hellas_sdk::serve_config::ServeConfig::load(&directory.path().join("serve.json"))
                .unwrap();
        let hellas_sdk::serve_config::Destination::Http { config } = &saved.routes[0].destination
        else {
            panic!("HTTP route changed driver")
        };
        assert_eq!(
            config.credentials["account"].trust_roots.as_ref(),
            Some(&roots)
        );
        assert_eq!(
            config.credentials["account"].source.secret_env.as_deref(),
            Some("GATE_TEST_UNUSED_TOKEN")
        );
    }
    #[tokio::test(flavor = "multi_thread")]
    async fn one_host_mounts_openai_and_https_and_refuses_updates_before_stopping() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = tempfile::tempdir().unwrap();
        let vault = Vault::fixture(secrets.path());
        let app = AppState::with_vault(directory.path(), vault.clone())
            .await
            .unwrap();
        app.initialize_software("Alice".into()).await.unwrap();
        assert!(app.set_provider(true).await.is_err());
        app.bind_provider(true).await.unwrap();
        app.bind_provider(true).await.unwrap();
        app.configure_provider(
            0,
            ProviderChange::Endpoint {
                service: "openai".into(),
                method: "responses".into(),
                hosts: vec!["api.openai.com".into()],
                allow_private_addresses: false,
            },
        )
        .await
        .unwrap();
        let local = app.local().await.unwrap();
        let daily = local.client(false).await.unwrap();
        let endpoint_change = update(
            1,
            ProviderChange::Endpoint {
                service: "lan".into(),
                method: "http".into(),
                hosts: vec!["example.com".into()],
                allow_private_addresses: false,
            },
        );
        assert_eq!(
            daily
                .configure_provider(ConfigureProviderRequest {
                    authorization: local.authorization(),
                    update: Some(endpoint_change)
                })
                .await
                .unwrap_err()
                .code(),
            hellas_wire::WireCode::PermissionDenied
        );
        declare_test_account(
            directory.path(),
            "openai",
            "responses",
            &["account"],
            "http://127.0.0.1:9/metrics",
        );
        app.configure_provider(
            1,
            ProviderChange::AddKey {
                key: UpstreamKeyInput {
                    service: "openai".into(),
                    method: "responses".into(),
                    key: "account".into(),
                    origins: vec!["https://api.openai.com".into()],
                    paths: vec!["/v1/responses".into()],
                    methods: vec!["POST".into()],
                    header: "authorization".into(),
                    prefix: "Bearer ".into(),
                    secret: "fixture-upstream-secret".into(),
                },
            },
        )
        .await
        .unwrap();
        app.configure_provider(
            2,
            ProviderChange::Endpoint {
                service: "lan".into(),
                method: "http".into(),
                hosts: vec!["example.com".into()],
                allow_private_addresses: false,
            },
        )
        .await
        .unwrap();
        app.set_provider(true).await.unwrap();
        app.configure_resource(
            3,
            ResourceInput {
                name: "responses".into(),
                service: "openai".into(),
                method: "responses".into(),
                resource: ResourceKind::OpenAi {
                    model: "fixture-model".into(),
                    max_output_tokens: 64,
                    credential: "account".into(),
                },
            },
        )
        .await
        .unwrap();
        app.configure_resource(
            4,
            ResourceInput {
                name: "pages".into(),
                service: "lan".into(),
                method: "http".into(),
                resource: ResourceKind::Https {
                    tls_roots: vec![],
                    origin: "https://example.com".into(),
                    paths: vec!["/".into()],
                    methods: vec!["GET".into()],
                    credential: None,
                    max_response_bytes: 4096,
                },
            },
        )
        .await
        .unwrap();
        assert_eq!(app.status().await.unwrap().provider.state, "running");
        // The server and factory reject both step-up and revision failures while running.
        let bad = update(
            5,
            ProviderChange::RemoveEndpoint {
                service: "lan".into(),
                method: "http".into(),
            },
        );
        assert!(
            daily
                .configure_provider(ConfigureProviderRequest {
                    authorization: local.authorization(),
                    update: Some(bad)
                })
                .await
                .is_err()
        );
        assert!(
            app.configure_provider(
                0,
                ProviderChange::RemoveEndpoint {
                    service: "lan".into(),
                    method: "http".into()
                }
            )
            .await
            .is_err()
        );
        assert_eq!(app.status().await.unwrap().provider.state, "running");
        let WorkControlReply::Catalogue(policies) =
            work(&daily, local.authorization(), WorkControlCommand::Catalogue)
                .await
                .unwrap()
        else {
            panic!("catalogue");
        };
        assert_eq!(policies.len(), 2);
        assert!(policies.iter().any(|p| p.responses.is_some()));
        assert!(policies.iter().any(|p| p.https.is_some()));
        let metadata = std::fs::read_to_string(directory.path().join("serve.json")).unwrap();
        assert!(!metadata.contains("fixture-upstream-secret"));
        app.shutdown().await.unwrap();
        drop(daily);
        drop(local);
        drop(app);
        let app = AppState::with_vault(directory.path(), vault).await.unwrap();
        assert_eq!(app.provider_view().await.unwrap().revision, 5);
        app.set_provider(true).await.unwrap();
        app.shutdown().await.unwrap();
    }
}

mod document;

pub(crate) fn configuration_path(directory: &Path) -> anyhow::Result<PathBuf> {
    document::path(directory)
}
