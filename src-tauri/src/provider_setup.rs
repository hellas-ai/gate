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
use hellas_providers::{
    HttpCredential, HttpEgressPolicy, HttpFetchAdaptorFactory, HttpFetchProvider,
};
use hellas_sdk::work::model::{
    fetch::{FetchPolicyV2, FetchRoutePolicy as WorkRoute, fetch_route_commitment},
    grant::{
        pricing::Pricing, records::GrantPolicy, resource::HttpsResource,
        responses::ResponsesResource,
    },
    profile::WorkPolicy,
};
use hellas_sdk::{
    FetchRoute, FetchRouteEntry, FetchRoutePolicy, FetchRouteRegistry,
    node_control::{NodeControl, configuration::*, host::*},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    num::NonZeroU64,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    revision: u64,
    paid: Option<PaidServeInput>,
    policy_revisions: BTreeMap<String, u64>,
    endpoints: BTreeMap<String, Endpoint>,
    resources: Vec<ResourceInput>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Endpoint {
    service: String,
    method: String,
    hosts: Vec<String>,
    private: bool,
    keys: BTreeMap<String, Key>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
        let config =
            vault::read(&directory.join("provider-configuration.json"))?.unwrap_or_default();
        Ok(Self {
            directory: directory.to_owned(),
            vault,
            state: Arc::new(Mutex::new(config)),
            sources: Arc::new(Mutex::new(BTreeMap::new())),
            #[cfg(test)]
            responses_origin: Default::default(),
        })
    }
    fn snapshot(&self) -> anyhow::Result<Configuration> {
        Ok(self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("provider configuration lock failed"))?
            .clone())
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
        vault::write(&self.directory.join("provider-configuration.json"), &config)?;
        *held = config;
        Ok(())
    }
    pub fn policy(&self, name: &str) -> anyhow::Result<GrantPolicy> {
        self.validate(&self.snapshot()?)?
            .into_iter()
            .find(|p| p.name == name)
            .context("resource is not in the composition catalogue")
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
        let resource = id("hellas.gate.resource.v1", &input.name);
        let previous = active.iter().find(|policy| policy.resource == resource);
        if let Some(previous) = previous {
            ensure!(
                previous.account
                    == id(
                        "hellas.gate.account.v1",
                        &route_id(&input.service, &input.method)
                    ),
                "a resource cannot move to another upstream account; choose a new resource name"
            );
        }
        let revision = previous
            .map_or(Some(1), |policy| policy.revision.0.checked_add(1))
            .context("resource revision overflow")?;
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
        if serde_json::to_vec(&next.paid)? == serde_json::to_vec(&input)? {
            return self.view();
        }
        next.paid = input;
        self.validate(&next)?;
        if let Some(input) = &next.paid {
            let mut config = crate::paid::serve_config(&self.directory, input)?;
            hellas_sdk::paid_provider::WorkRunnerConfig {
                network: config.chain.network,
                journal_root: config.journal_root.clone(),
                routes: config.routes.clone(),
                poll: config.poll,
                max_observation_age: config.max_observation_age,
                settlement_key: hellas_sdk::kernel::Secp256k1Signer::from_secret_scalar(
                    local.identity.node.node_secret_bytes(),
                )
                .map_err(|_| anyhow::anyhow!("invalid provider settlement key"))?,
                policy: config.provider_policy(),
            }
            .validate()?;
            // Live setup journals have one exclusive owner. Unchanged bond/client
            // bindings were checked at startup; validate only newly selected ones.
            let previous = self.snapshot()?.paid;
            config.routes = hellas_sdk::work_config::WorkRoutes::try_from_iter(
                config
                    .routes
                    .iter()
                    .filter(|route| {
                        !previous.as_ref().is_some_and(|old| {
                            old.routes.iter().any(|r| {
                                r.bond == hex::encode(route.bond.to_bytes())
                                    && r.client == hex::encode(route.client.to_bytes())
                            })
                        })
                    })
                    .cloned()
                    .collect::<Vec<_>>(),
            )?;
            hellas_sdk::work_config::validate_work_routes(&config).await?;
            local.chain.get(&config).await?;
        }
        let status = session.status().await?;
        let running = status.provider.is_some_and(|p| p.state == 2);
        next.revision = expected
            .checked_add(1)
            .context("provider revision overflow")?;
        session.track_local("Publish typed paid Serve settings".into(), || {
            self.publish(expected, next)
        })?;
        if running {
            session.provider_state(false).await?;
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
            vault::write(&self.directory.join("provider-configuration.json"), &next)?;
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
    fn validate(&self, config: &Configuration) -> anyhow::Result<Vec<GrantPolicy>> {
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
            let mut policy = match &resource.resource {
                ResourceKind::OpenAi {
                    model,
                    max_output_tokens,
                    credential,
                } => {
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
                    hellas_reseller::responses_policy(
                        resource_id,
                        account,
                        revision,
                        &resource.service,
                        &resource.method,
                        ResponsesResource {
                            model: model.clone(),
                            max_output_tokens: *max_output_tokens,
                            settings: Default::default(),
                        },
                        pricing,
                    )?
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
                    let https = HttpsResource {
                        origin: origin.clone(),
                        paths: paths.clone(),
                        methods: methods.clone(),
                        credential: credential.clone(),
                        tls: hellas_rpc::http_fetch::HttpTls {
                            roots: if tls_roots.is_empty() {
                                hellas_rpc::http_fetch::HttpTrustRoots::WebPki
                            } else {
                                hellas_rpc::http_fetch::HttpTrustRoots::Certificates {
                                    der_base64: tls_roots.clone(),
                                }
                            },
                            spki_sha256: vec![],
                        },
                        accounting: hellas_sdk::work::model::http_usage::AccountingProfile::None,
                        max_output_tokens: 0,
                        max_response_bytes: *max_response_bytes,
                    };
                    https.validate()?;
                    let route = WorkRoute::sealed_route(&resource.service, &resource.method)?;
                    let contract = hellas_rpc::FetchContract::http()?;
                    GrantPolicy {
                        resource: resource_id,
                        account,
                        revision,
                        name: resource.name.clone(),
                        work: WorkPolicy::Fetch {
                            policy: FetchPolicyV2 {
                                allowed_environment: contract.manifest_id(),
                                route_commitment: fetch_route_commitment(
                                    &route.canonical_body_bytes(),
                                )?,
                                max_request_body_bytes: 64 << 10,
                                max_output_events: 4096,
                                max_output_bytes: 4 << 20,
                                max_spool_bytes: 8 << 20,
                                max_encoded_result_frame: 4 << 20,
                                max_encoded_prepared_input: 128 << 10,
                            },
                            route,
                            contract,
                        },
                        https: Some(https),
                        responses: None,
                        pricing,
                    }
                }
            };
            policy.name = resource.name.clone();
            policy.validate()?;
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
        let policies = self.validate(&config)?;
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
        let mut routes = FetchRouteRegistry::new();
        for endpoint in config.endpoints.values() {
            let resource = config
                .resources
                .iter()
                .find(|r| r.service == endpoint.service && r.method == endpoint.method);
            let entry = if let Some(ResourceInput {
                resource: ResourceKind::OpenAi { credential, .. },
                ..
            }) = resource
            {
                let key = endpoint
                    .keys
                    .get(credential)
                    .context("upstream key missing")?;
                let bearer = self.vault.source(&key.reference, "").resolve().await?;
                let driver: Arc<dyn hellas_executor::FetchProvider> = Arc::new(
                    hellas_reseller::OpenAiResponsesFetchProvider::with_bearer(bearer)?,
                );
                #[cfg(test)]
                let driver = match self.responses_origin.lock().unwrap().as_ref() {
                    Some(origin) => crate::responses_fixture::provider(origin),
                    None => driver,
                };
                FetchRouteEntry::new(
                    crate::usage::track(
                        driver,
                        self.clone(),
                        crate::usage::Key::Fixed(key.reference.clone(), key.fingerprint.clone()),
                    ),
                    Arc::new(hellas_reseller::OpenAiResponsesAdaptorFactory::new()?),
                    FetchRoutePolicy::default(),
                )?
            } else {
                FetchRouteEntry::new(
                    crate::usage::track(
                        Arc::new(self.http(endpoint)?),
                        self.clone(),
                        crate::usage::Key::Http(
                            endpoint
                                .keys
                                .iter()
                                .map(|(alias, key)| {
                                    (
                                        alias.clone(),
                                        (key.reference.clone(), key.fingerprint.clone()),
                                    )
                                })
                                .collect(),
                        ),
                    ),
                    Arc::new(HttpFetchAdaptorFactory::new()?),
                    FetchRoutePolicy::default(),
                )?
            };
            routes.register(FetchRoute::new(&endpoint.service, &endpoint.method), entry)?;
        }
        let options = hellas_sdk::FetchProviderOptions {
            port: None,
            identity: local.identity.node.clone(),
            enrollment: identity.enrollment,
            root: identity.root,
            state_directory: self.directory.join("provider"),
            grants: Some(hellas_sdk::GrantProviderOptions {
                policies,
                max_job_millis: NonZeroU64::new(300_000).context("invalid job duration")?,
            }),
            routes,
            fetch_max_in_flight: hellas_rpc::DEFAULT_FETCH_MAX_IN_FLIGHT,
            fetch_queue_capacity: hellas_rpc::DEFAULT_FETCH_QUEUE_CAPACITY,
            paid_work: if let Some(input) = &config.paid {
                let paid = crate::paid::serve_config(&self.directory, input)?;
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
    use hellas_rpc::pb::host::{GetHostStatusRequest, SetMachineCapacityRequest};
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
        .work(hellas_sdk::work::model::grant::admin::WorkControlCommand::Catalogue)
        .await?
    {
        hellas_sdk::work::model::grant::admin::WorkControlReply::Catalogue(policies) => policies,
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
    if status.machine.as_ref().is_none_or(|m| m.max_in_flight == 0) {
        session
            .track(
                "Configure machine capacity".into(),
                true,
                client.set_machine_capacity(SetMachineCapacityRequest {
                    authorization: local.authorization(),
                    expected_revision: status.machine.map_or(0, |m| m.revision),
                    max_in_flight: 128,
                    limits: hellas_rpc::protocol::value::canonical_dag_cbor(&Vec::<
                        hellas_sdk::work::model::grant::budget::Limit,
                    >::new(
                    ))?,
                }),
            )
            .await?;
    }
    for name in names {
        let policy = local.composition.policy(&name)?;
        use hellas_sdk::work::model::grant::{admin::*, budget::*};
        let account = BudgetNode::Account(policy.account);
        if session
            .work(WorkControlCommand::InspectBudget { node: account })
            .await
            .is_err()
        {
            session
                .work(WorkControlCommand::ConfigureBudget {
                    definition: BudgetDef {
                        node: account,
                        parent: BudgetNode::Machine,
                        revision: hellas_auth::authority::Revision(1),
                        limits: vec![],
                        max_in_flight: std::num::NonZeroU16::new(128)
                            .context("invalid account capacity")?,
                    },
                })
                .await
                .context("configuring the upstream account budget")?;
        }
        let configured = session.work(WorkControlCommand::Catalogue).await?;
        if !matches!(configured, WorkControlReply::Catalogue(ref entries) if entries.contains(&policy))
        {
            session
                .work(WorkControlCommand::ConfigureResource { policy: name })
                .await
                .context("activating the resource in Work authority")?;
        }
    }
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
mod tests {
    use super::*;
    use crate::control::work;
    use crate::state::AppState;
    use hellas_rpc::pb::host::*;
    use hellas_sdk::work::model::grant::admin::{WorkControlCommand, WorkControlReply};
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
        let metadata =
            std::fs::read_to_string(directory.path().join("provider-configuration.json")).unwrap();
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
