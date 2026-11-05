//! SDK configuration at rest; the existing typed controls are only an editing view.
use super::*;
use hellas_providers::{
    HttpCredentialConfig, HttpProviderConfig, HttpSecretConfig, HttpSecretStore,
};
use hellas_sdk::grant_config::{AccountFile, GrantSettings, ResourceFile};
use hellas_sdk::serve_config::{CredentialMetadata, Destination, PaidTerms, Route, ServeConfig};

pub(super) fn path(directory: &Path) -> anyhow::Result<PathBuf> {
    match std::env::var_os("HELLAS_GATE_CONFIG") {
        Some(path) => Ok(std::path::absolute(path)?),
        None => Ok(directory.join("serve.json")),
    }
}
pub(super) fn read(path: &Path) -> anyhow::Result<ServeConfig> {
    ServeConfig::load(path)
}

pub(super) fn policy(resource: &ResourceFile) -> GrantPolicy {
    GrantPolicy {
        resource: resource.resource,
        account: resource.account,
        revision: resource.revision,
        name: resource.name.clone(),
        invocation_policy: resource.invocation_policy.clone(),
        https: resource.https.clone(),
        responses: resource.responses.clone(),
        pricing: resource.pricing.clone(),
    }
}
fn reference(source: &HttpSecretConfig) -> String {
    match &source.secret_store {
        Some(HttpSecretStore::Native { key, .. } | HttpSecretStore::HeadlessFiles { key, .. }) => {
            key.clone()
        }
        None => match (&source.secret_env, &source.secret_file) {
            (Some(name), _) => format!("env:{name}"),
            (_, Some(path)) => format!("file:{}", path.display()),
            _ => String::new(),
        },
    }
}
fn key(source: &HttpCredentialConfig, metadata: Option<&CredentialMetadata>) -> Key {
    Key {
        origins: source.allowed_origins.clone(),
        paths: source.allowed_paths.clone(),
        methods: source.allowed_methods.clone(),
        header: source.header_name.clone(),
        prefix: source.source.prefix.clone(),
        reference: reference(&source.source),
        fingerprint: metadata.map(|m| m.fingerprint.clone()).unwrap_or_default(),
        last_use: metadata.and_then(|m| m.last_use),
    }
}
fn http_credential(key: &Key, source: HttpSecretConfig) -> HttpCredentialConfig {
    HttpCredentialConfig {
        allowed_origins: key.origins.clone(),
        allowed_paths: key.paths.clone(),
        allowed_methods: key.methods.clone(),
        trust_roots: None,
        header_name: key.header.clone(),
        source,
    }
}
pub(super) fn view(doc: &ServeConfig, directory: &Path) -> anyhow::Result<Configuration> {
    let mut config = Configuration {
        revision: doc.revision,
        ..Default::default()
    };
    for route in &doc.routes {
        let (hosts, private, keys) = match &route.destination {
            Destination::Http { config } => (
                config.allowed_hosts.clone(),
                config.allow_private_addresses,
                config
                    .credentials
                    .iter()
                    .map(|(name, source)| {
                        (
                            name.clone(),
                            key(source, route.credential_metadata.get(name)),
                        )
                    })
                    .collect(),
            ),
            Destination::OpenAiResponses { credentials, .. } => (
                vec!["api.openai.com".into()],
                false,
                credentials
                    .iter()
                    .map(|(name, credential)| {
                        let metadata = route.credential_metadata.get(name);
                        (
                            name.clone(),
                            Key {
                                origins: vec!["https://api.openai.com".into()],
                                paths: vec!["/v1/responses".into()],
                                methods: vec!["POST".into()],
                                header: "authorization".into(),
                                prefix: "Bearer ".into(),
                                reference: reference(credential),
                                fingerprint: metadata
                                    .map(|m| m.fingerprint.clone())
                                    .unwrap_or_default(),
                                last_use: metadata.and_then(|m| m.last_use),
                            },
                        )
                    })
                    .collect(),
            ),
            Destination::CodexResponses { .. } => {
                (vec!["chatgpt.com".into()], false, BTreeMap::new())
            }
        };
        config.endpoints.insert(
            route_id(&route.service, &route.method),
            Endpoint {
                service: route.service.clone(),
                method: route.method.clone(),
                hosts,
                private,
                keys,
            },
        );
    }
    if let Some(grants) = &doc.grants {
        for resource in &grants.resources {
            let Some(Admission::Fetch { admission }) =
                resource.invocation_policy.admissions.first()
            else {
                continue;
            };
            let (service, method) = match &admission.route {
                WorkRoute::SealedRoute { service, method } => (service.clone(), method.clone()),
                WorkRoute::OpenFetch { .. } => ("http".into(), "request".into()),
            };
            let kind = if let Some(https) = &resource.https {
                ResourceKind::Https {
                    origin: https.origin.clone(),
                    paths: https.paths.clone(),
                    methods: https.methods.clone(),
                    credential: https.credential.clone(),
                    max_response_bytes: https.max_response_bytes,
                    tls_roots: match &https.tls.roots {
                        hellas_rpc::http_fetch::HttpTrustRoots::WebPki => vec![],
                        hellas_rpc::http_fetch::HttpTrustRoots::Certificates { der_base64 } => {
                            der_base64.clone()
                        }
                    },
                }
            } else if let Some(responses) = &resource.responses {
                let Some(route) = doc
                    .routes
                    .iter()
                    .find(|r| r.service == service && r.method == method)
                else {
                    continue;
                };
                let Destination::OpenAiResponses {
                    credential: credential_name,
                    ..
                } = &route.destination
                else {
                    continue;
                };
                ResourceKind::OpenAi {
                    model: responses.model.clone(),
                    max_output_tokens: responses.max_output_tokens,
                    credential: credential_name.clone(),
                }
            } else {
                continue;
            };
            config
                .policy_revisions
                .insert(resource.name.clone(), resource.revision.0);
            config.resources.push(ResourceInput {
                name: resource.name.clone(),
                service,
                method,
                resource: kind,
            });
        }
    }
    if let Some(terms) = &doc.paid
        && let Some(Admission::Fetch { admission: fetch }) =
            terms.invocation_policy.admissions.first()
        && let WorkRoute::SealedRoute { service, method } = &fetch.route
    {
        let chain = match &terms.chain {
            Some(c) => c.clone().into_config()?,
            None => hellas_sdk::client_chain::devnet(&directory.join("provider"))?,
        };
        let genesis = chain.chain.genesis.as_ref();
        let presentation =
            if fetch.contract.schema() == hellas_rpc::FetchSchema::StatelessResponsesV1 {
                FetchPresentation::Responses
            } else {
                FetchPresentation::Http
            };
        config.paid = Some(PaidServeInput {
            policy: PaidPolicyInput {
                chain: ChainInput {
                    network: chain.chain.network.as_str().into(),
                    genesis_digest: chain.chain.genesis_payload_digest.to_string(),
                    threshold_identity: hex::encode(&chain.chain.threshold_identity),
                    validators: chain.validators,
                    committee: genesis
                        .map(|g| {
                            g.validators
                                .iter()
                                .map(|v| GenesisValidatorInput {
                                    public_key: v.public_key.clone(),
                                    label: v.label.clone(),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    allocations: genesis
                        .map(|g| {
                            g.allocations
                                .iter()
                                .map(|v| GenesisAllocationInput {
                                    address: v.address.clone(),
                                    balance: v.balance,
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                },
                service: service.clone(),
                method: method.clone(),
                presentation,
                compute_credit_limit: terms.channel.compute_credit_limit,
                delivery_credit_limit: terms.channel.delivery_credit_limit,
                fixed_price: terms.payment.fixed_price,
                dispatch_blocks: terms.payment.dispatch_margin_blocks,
                delivery_blocks: terms.payment.delivery_margin_blocks,
                oracle_blocks: terms.payment.oracle_grace_blocks,
                jobs_per_channel: terms.jobs_per_channel,
                omission_bond: terms.offers.omission_bond,
                max_stake_per_client: terms.offers.max_stake_per_client,
                max_open_offers: terms.offers.max_open_offers,
                bond_lifetime_blocks: terms.offers.bond_lifetime_blocks,
                redelegate_payout: terms.offers.redelegate_payout,
                limits: PaidFetchLimitsInput {
                    max_request_body_bytes: fetch.limits.max_request_body_bytes,
                    max_output_events: fetch.limits.max_output_events,
                    max_output_bytes: terms.invocation_policy.limits.max_output_bytes,
                    max_spool_bytes: fetch.limits.max_spool_bytes,
                    max_encoded_result_frame: fetch.limits.max_encoded_result_frame,
                    max_encoded_prepared_input: fetch.limits.max_encoded_prepared_input,
                    execution_timeout_ms: terms.invocation_policy.limits.execution_timeout_ms,
                },
                min_omit_response_blocks: terms.min_omit_response_blocks,
                poll_ms: chain.poll.as_millis().try_into()?,
                max_observation_age_ms: chain.max_observation_age.as_millis().try_into()?,
            },
        });
    }
    Ok(config)
}

pub(super) fn write(
    host: &Composition,
    config: &Configuration,
    previous: &Configuration,
) -> anyhow::Result<()> {
    let mut doc = read(&host.config_path)?;
    ensure!(
        doc.revision == previous.revision,
        "host configuration changed on disk; reopen before editing"
    );
    // Only the controlled editing view is changed. Unrepresented destinations and
    // policy fields remain in the canonical document until explicitly changed.
    doc.revision = config.revision;
    for endpoint in config.endpoints.values() {
        let position = doc
            .routes
            .iter()
            .position(|r| r.service == endpoint.service && r.method == endpoint.method)
            .unwrap_or(doc.routes.len());
        if position == doc.routes.len() {
            doc.routes.push(Route {
                service: endpoint.service.clone(),
                method: endpoint.method.clone(),
                destination: Destination::Http {
                    config: HttpProviderConfig::default(),
                },
                account: None,
                credential_accounts: BTreeMap::new(),
                capabilities: Default::default(),
                credential_metadata: BTreeMap::new(),
            });
        }
        let route = &mut doc.routes[position];
        match &mut route.destination {
            Destination::Http { config: http } => {
                http.allowed_hosts = endpoint.hosts.clone();
                http.allow_private_addresses = endpoint.private;
                http.credentials
                    .retain(|name, _| endpoint.keys.contains_key(name));
                for (name, key) in &endpoint.keys {
                    let credential = http.credentials.entry(name.clone()).or_insert_with(|| {
                        http_credential(key, host.vault.secret_config(&key.reference, &key.prefix))
                    });
                    if reference(&credential.source) != key.reference {
                        credential.source = host.vault.secret_config(&key.reference, &key.prefix);
                    }
                    credential.allowed_origins = key.origins.clone();
                    credential.allowed_paths = key.paths.clone();
                    credential.allowed_methods = key.methods.clone();
                    credential.header_name = key.header.clone();
                    credential.source.prefix = key.prefix.clone();
                }
            }
            Destination::OpenAiResponses { credentials, .. } => {
                ensure!(
                    endpoint.hosts == ["api.openai.com"] && !endpoint.private,
                    "sealed OpenAI endpoint cannot change hosts"
                );
                credentials.retain(|name, _| endpoint.keys.contains_key(name));
                for (name, key) in &endpoint.keys {
                    let credential = credentials
                        .entry(name.clone())
                        .or_insert_with(|| host.vault.secret_config(&key.reference, ""));
                    if reference(credential) != key.reference {
                        *credential = host.vault.secret_config(&key.reference, "");
                    }
                    ensure!(
                        key.prefix == "Bearer ",
                        "sealed OpenAI uses the Bearer header prefix"
                    );
                }
            }
            Destination::CodexResponses { .. } => {
                ensure!(
                    endpoint.hosts == ["chatgpt.com"]
                        && !endpoint.private
                        && endpoint.keys.is_empty(),
                    "sealed Codex destination cannot be edited as HTTP"
                );
            }
        }
        route.credential_metadata = endpoint
            .keys
            .iter()
            .map(|(name, key)| {
                (
                    name.clone(),
                    CredentialMetadata {
                        fingerprint: key.fingerprint.clone(),
                        last_use: key.last_use,
                    },
                )
            })
            .collect();
    }
    doc.routes.retain(|r| {
        config
            .endpoints
            .contains_key(&route_id(&r.service, &r.method))
    });
    // Only an explicit resource edit can select a different driver/credential.
    for resource in &config.resources {
        if previous.resources.iter().any(|r| r == resource) {
            continue;
        }
        let route = doc
            .routes
            .iter_mut()
            .find(|r| r.service == resource.service && r.method == resource.method)
            .context("resource route missing")?;
        if let ResourceKind::OpenAi {
            credential,
            max_output_tokens,
            ..
        } = &resource.resource
        {
            match &mut route.destination {
                Destination::Http { config: http } => {
                    ensure!(
                        http.credentials.contains_key(credential),
                        "upstream credential missing"
                    );
                    route.destination = Destination::OpenAiResponses {
                        credential: credential.clone(),
                        credentials: http
                            .credentials
                            .iter()
                            .map(|(name, key)| {
                                let mut source = key.source.clone();
                                source.prefix.clear();
                                (name.clone(), source)
                            })
                            .collect(),
                    };
                }
                Destination::OpenAiResponses {
                    credential: selected,
                    credentials,
                } => {
                    ensure!(
                        credentials.contains_key(credential),
                        "upstream credential missing"
                    );
                    *selected = credential.clone();
                }
                Destination::CodexResponses { .. } => {
                    anyhow::bail!("Codex route cannot become OpenAI")
                }
            }
            if route.capabilities.max_output_tokens.is_none() {
                route.capabilities.max_output_tokens = Some(u64::from(*max_output_tokens));
            }
        } else if let Destination::OpenAiResponses { credentials, .. } = &route.destination {
            let endpoint = config
                .endpoints
                .get(&route_id(&route.service, &route.method))
                .context("endpoint missing")?;
            route.destination = Destination::Http {
                config: HttpProviderConfig {
                    allowed_hosts: endpoint.hosts.clone(),
                    allow_private_addresses: endpoint.private,
                    credentials: credentials
                        .iter()
                        .map(|(name, source)| {
                            let key = &endpoint.keys[name];
                            (
                                name.clone(),
                                http_credential(
                                    key,
                                    HttpSecretConfig {
                                        prefix: format!("{}{}", key.prefix, source.prefix),
                                        ..source.clone()
                                    },
                                ),
                            )
                        })
                        .collect(),
                },
            };
        }
    }
    let policies = host.validate(config)?;
    let default_budget = hellas_authority::model::grant::configuration::ConfiguredBudget {
        max_in_flight: std::num::NonZeroU16::new(128).context("invalid capacity")?,
        limits: vec![],
    };
    let mut grants = doc.grants.take().unwrap_or(GrantSettings {
        journal_root: None,
        control_socket: None,
        control_certificate: None,
        control_private_key: None,
        remote_control_peers: vec![],
        remote_control: false,
        max_job_millis: NonZeroU64::new(300000).context("invalid duration")?,
        machine: Some(default_budget.clone()),
        accounts: vec![],
        resources: vec![],
    });
    grants.resources.retain(|r| {
        !previous.resources.iter().any(|v| v.name == r.name)
            || config.resources.iter().any(|v| v.name == r.name)
    });
    for policy in policies {
        grants.resources.retain(|r| r.name != policy.name);
        if !grants.accounts.iter().any(|a| a.account == policy.account) {
            grants.accounts.push(AccountFile {
                account: policy.account,
                max_in_flight: default_budget.max_in_flight,
                limits: vec![],
            });
        }
        grants.resources.push(policy);
    }
    doc.grants = Some(grants);
    if serde_json::to_vec(&config.paid)? != serde_json::to_vec(&previous.paid)? {
        doc.paid = config
            .paid
            .as_ref()
            .map(|input| -> anyhow::Result<_> {
                let policy = crate::paid::standing_policy(&input.policy)?;
                let chain = crate::paid::serve_config(&host.directory, input)?;
                let serialized_chain = hellas_sdk::work_config::WorkConfigFile {
                    chain: hellas_sdk::work_config::ChainFile {
                        network_id: chain.chain.network.as_str().into(),
                        genesis_payload_digest: hex::encode(
                            chain.chain.genesis_payload_digest.as_bytes(),
                        ),
                        threshold_identity: hex::encode(chain.chain.threshold_identity),
                        genesis: chain.chain.genesis,
                    },
                    validators: chain.validators,
                    journal: hellas_sdk::work_config::JournalFile {
                        root: chain.journal_root,
                    },
                    poll_ms: chain.poll.as_millis().try_into()?,
                    max_observation_age_ms: chain.max_observation_age.as_millis().try_into()?,
                };
                Ok(PaidTerms {
                    chain: Some(serialized_chain),
                    jobs_per_channel: policy.jobs_per_channel,
                    min_omit_response_blocks: policy.min_omit_response_blocks,
                    payment: policy.payment_policy,
                    channel: policy.channel_policy,
                    offers: policy.offers,
                    invocation_policy: policy.invocation_policy,
                })
            })
            .transpose()?;
    }
    doc.validate()?;
    vault::write(&host.config_path, &doc)
}
