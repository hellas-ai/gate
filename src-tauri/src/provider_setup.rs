//! Provider editing translates Watch into one atomic Configure document.
use crate::{
    control::{LocalNode, Session},
    dto::*,
};
use anyhow::{Context, ensure};
use hellas_auth::authority::{ResourceId, Revision};
use hellas_authority::model::grant::{
    configuration::{ConfiguredBudget, NodeConfiguration},
    pricing::Pricing,
    records::GrantPolicy,
    resource::HttpsResource,
    responses::ResponsesResource,
};
use hellas_rpc::protocol::invocation::{
    Admission, InvocationPolicy, Limits,
    fetch::{FetchAdmission, FetchLimits, FetchRoutePolicy as WorkRoute},
};
use hellas_sdk::daemon_config::{Backend, BackendKind, Credential, Document, Listener, Upstream};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::{NonZeroU16, NonZeroU64},
};

pub fn route_id(service: &str, method: &str) -> String {
    format!("{service}/{method}")
}
fn id(domain: &str, name: &str) -> ResourceId {
    ResourceId(hellas_auth::hash_tuple(domain, &[name.as_bytes()]))
}
pub async fn view(local: &LocalNode) -> anyhow::Result<ProviderView> {
    let snapshot = local.session(false).await?.view().await?;
    view_document(&snapshot.document)
}
pub fn view_document(document: &Document) -> anyhow::Result<ProviderView> {
    let upstreams = document
        .backends
        .values()
        .filter_map(|backend| {
            let BackendKind::Fetch {
                service,
                method,
                upstream,
            } = &backend.kind
            else {
                return None;
            };
            let (hosts, keys) = match upstream {
                Upstream::Http {
                    allowed_hosts,
                    credentials,
                    ..
                } => (allowed_hosts.clone(), credentials.keys().cloned().collect()),
                Upstream::OpenaiResponses { .. } => (vec!["api.openai.com".into()], vec![]),
                Upstream::OpenaiHttp { url, .. } => (vec![url.clone()], vec![]),
            };
            Some(UpstreamView {
                service: service.clone(),
                method: method.clone(),
                hosts,
                keys,
            })
        })
        .collect();
    let resources = document
        .authority
        .resources()
        .iter()
        .map(input)
        .collect::<anyhow::Result<_>>()?;
    Ok(ProviderView {
        revision: document.revision.0,
        upstreams,
        resources,
    })
}
fn input(policy: &GrantPolicy) -> anyhow::Result<ResourceInput> {
    let [Admission::Fetch { admission }] = policy.invocation_policy.admissions.as_slice() else {
        anyhow::bail!("resource is not Fetch")
    };
    let WorkRoute::SealedRoute { service, method } = &admission.route else {
        anyhow::bail!("resource has no named route")
    };
    let resource = if let Some(responses) = &policy.responses {
        ResourceKind::OpenAi {
            model: responses.model.clone(),
            max_output_tokens: responses.max_output_tokens,
        }
    } else {
        let https = policy
            .https
            .as_ref()
            .context("unsupported Fetch resource")?;
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
    };
    Ok(ResourceInput {
        name: policy.name.clone(),
        service: service.clone(),
        method: method.clone(),
        resource,
    })
}
pub async fn policy(local: &LocalNode, name: &str) -> anyhow::Result<GrantPolicy> {
    local
        .session(false)
        .await?
        .view()
        .await?
        .document
        .authority
        .resource(name)
        .cloned()
        .context("resource is not configured")
}
pub fn change(document: &mut Document, change: ProviderChange) -> anyhow::Result<()> {
    match change {
        ProviderChange::OpenAi {
            service,
            method,
            secret,
        } => {
            put(
                document,
                service,
                method,
                Upstream::OpenaiResponses { secret },
            )?;
        }
        ProviderChange::Endpoint {
            service,
            method,
            hosts,
            allow_private_addresses,
        } => {
            let credentials = match document
                .backends
                .get(&route_id(&service, &method))
                .map(|b| &b.kind)
            {
                Some(BackendKind::Fetch {
                    upstream: Upstream::Http { credentials, .. },
                    ..
                }) => credentials.clone(),
                _ => BTreeMap::new(),
            };
            put(
                document,
                service,
                method,
                Upstream::Http {
                    allowed_hosts: hosts,
                    allow_private_addresses,
                    credentials,
                },
            )?;
        }
        ProviderChange::RemoveEndpoint { service, method } => {
            document
                .remove_backend(&route_id(&service, &method))
                .map_err(anyhow::Error::from_boxed)?;
        }
        ProviderChange::SetKey { key } => {
            let BackendKind::Fetch {
                upstream: Upstream::Http { credentials, .. },
                ..
            } = &mut document
                .backends
                .get_mut(&route_id(&key.service, &key.method))
                .context("select an HTTP backend")?
                .kind
            else {
                anyhow::bail!("select an HTTP backend")
            };
            credentials.insert(
                key.key,
                Credential {
                    trust_roots: hellas_rpc::http_fetch::HttpTrustRoots::WebPki,
                    allowed_origins: key.origins,
                    allowed_paths: key.paths,
                    allowed_methods: key.methods,
                    header_name: key.header,
                    secret: Some(format!("{}{}", key.prefix, key.secret)),
                },
            );
        }
        ProviderChange::RemoveKey {
            service,
            method,
            key,
        } => {
            let BackendKind::Fetch {
                upstream: Upstream::Http { credentials, .. },
                ..
            } = &mut document
                .backends
                .get_mut(&route_id(&service, &method))
                .context("select an HTTP backend")?
                .kind
            else {
                anyhow::bail!("select an HTTP backend")
            };
            ensure!(credentials.remove(&key).is_some(), "unknown key");
        }
    }
    sync_listener(document);
    // Keep authority and upstreams consistent in the same candidate.
    for resource in view_document(document)?.resources {
        build_policy(document, &resource, Revision(1))?;
    }
    Ok(())
}
fn put(
    document: &mut Document,
    service: String,
    method: String,
    upstream: Upstream,
) -> anyhow::Result<()> {
    WorkRoute::sealed_route(&service, &method)?;
    let name = route_id(&service, &method);
    let max_in_flight = document
        .backends
        .get(&name)
        .map_or(8.try_into()?, |b| b.max_in_flight);
    document.backends.insert(
        name,
        Backend {
            max_in_flight,
            kind: BackendKind::Fetch {
                service,
                method,
                upstream,
            },
        },
    );
    Ok(())
}
pub fn sync_listener(document: &mut Document) {
    if let Some(Listener::Iroh { backends, .. }) = document.listeners.get_mut("work") {
        *backends = document
            .backends
            .iter()
            .filter(|(_, b)| {
                matches!(
                    b.kind,
                    BackendKind::Fetch { .. } | BackendKind::Local { .. }
                )
            })
            .map(|(id, _)| id.clone())
            .collect();
    }
}
pub fn resources(
    document: &mut Document,
    inputs: Vec<ResourceInput>,
    catalogue: &[GrantPolicy],
) -> anyhow::Result<()> {
    let mut names = BTreeSet::new();
    let mut policies = vec![];
    for resource in inputs {
        ensure!(
            names.insert(resource.name.clone()),
            "duplicate resource name"
        );
        let previous = catalogue
            .iter()
            .find(|p| p.resource == id("hellas.gate.resource.v1", &resource.name));
        let mut policy = build_policy(
            document,
            &resource,
            previous.map_or(Revision(1), |p| p.revision),
        )?;
        if let Some(previous) = previous {
            policy.config_change = previous.config_change;
            ensure!(
                policy.account == previous.account,
                "a resource cannot move to another account"
            );
            if policy != *previous {
                policy.revision = previous
                    .revision
                    .next()
                    .context("resource revision overflow")?;
                policy.pricing.revision = policy.revision;
            }
        }
        policies.push(policy);
    }
    let capacity = NonZeroU16::new(256).context("invalid account capacity")?;
    let previous: BTreeMap<_, _> = document.authority.accounts().cloned().collect();
    let accounts = policies
        .iter()
        .map(|p| {
            (
                p.account,
                previous
                    .get(&p.account)
                    .cloned()
                    .unwrap_or(ConfiguredBudget {
                        max_in_flight: capacity,
                        limits: vec![],
                    }),
            )
        })
        .collect();
    let machine = document.authority.machine().cloned().or_else(|| {
        (!policies.is_empty()).then(|| ConfiguredBudget {
            max_in_flight: capacity,
            limits: vec![],
        })
    });
    document.authority = NodeConfiguration::new(machine, accounts, policies)?;
    Ok(())
}
fn build_policy(
    document: &Document,
    resource: &ResourceInput,
    revision: Revision,
) -> anyhow::Result<GrantPolicy> {
    let BackendKind::Fetch { upstream, .. } = &document
        .backends
        .get(&route_id(&resource.service, &resource.method))
        .context("resource backend is missing")?
        .kind
    else {
        anyhow::bail!("resource backend is not Fetch")
    };
    let account = id(
        "hellas.gate.account.v1",
        &route_id(&resource.service, &resource.method),
    );
    let resource_id = id("hellas.gate.resource.v1", &resource.name);
    let pricing = Pricing {
        revision,
        rates: None,
    };
    let mut policy = match &resource.resource {
        ResourceKind::OpenAi {
            model,
            max_output_tokens,
        } => {
            ensure!(
                matches!(upstream, Upstream::OpenaiResponses { .. }),
                "select a Responses backend"
            );
            hellas_share::responses_policy(hellas_share::ResponsesPolicyOptions {
                resource: resource_id,
                account,
                revision,
                service: &resource.service,
                method: &resource.method,
                responses: ResponsesResource {
                    model: model.clone(),
                    max_output_tokens: *max_output_tokens,
                    settings: Default::default(),
                },
                pricing,
                max_job_millis: NonZeroU64::new(300_000).context("invalid job duration")?,
            })?
        }
        ResourceKind::Https {
            origin,
            paths,
            methods,
            credential,
            max_response_bytes,
            tls_roots,
        } => {
            let Upstream::Http { credentials, .. } = upstream else {
                anyhow::bail!("select an HTTP backend")
            };
            if let Some(key) = credential {
                ensure!(
                    credentials.contains_key(key),
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
                accounting: hellas_authority::model::http_usage::AccountingProfile::None,
                max_output_tokens: 0,
                max_response_bytes: *max_response_bytes,
                model_policy: None,
            };
            https.validate()?;
            let route = WorkRoute::sealed_route(&resource.service, &resource.method)?;
            let contract = hellas_rpc::FetchContract::http()?;
            GrantPolicy {
                config_change: Default::default(),
                resource: resource_id,
                account,
                revision,
                name: resource.name.clone(),
                invocation_policy: InvocationPolicy {
                    admissions: vec![Admission::Fetch {
                        admission: FetchAdmission {
                            route,
                            contract,
                            limits: FetchLimits {
                                max_request_body_bytes: 64 << 10,
                                max_output_events: 4096,
                                max_spool_bytes: 8 << 20,
                                max_encoded_result_frame: 4 << 20,
                                max_encoded_prepared_input: 128 << 10,
                            },
                        },
                    }],
                    limits: Limits {
                        max_upload_bytes: 128 << 10,
                        max_output_bytes: 4 << 20,
                        max_steps: 1,
                        execution_timeout_ms: 300_000,
                    },
                },
                https: Some(https),
                responses: None,
                pricing,
            }
        }
    };
    policy.name = resource.name.clone();
    policy.validate()?;
    Ok(policy)
}
pub async fn configure(
    local: &LocalNode,
    session: &Session,
    expected: u64,
    edit: ProviderChange,
) -> anyhow::Result<ProviderView> {
    let mut document = session.view().await?.document;
    ensure!(
        document.revision.0 == expected,
        "provider revision conflict"
    );
    change(&mut document, edit)?;
    session.configure(document).await?;
    view(local).await
}
pub async fn configure_resource(
    local: &LocalNode,
    session: &Session,
    expected: u64,
    resource: ResourceInput,
) -> anyhow::Result<ProviderView> {
    configure_resources(local, session, expected, vec![resource]).await
}
pub async fn configure_resources(
    local: &LocalNode,
    session: &Session,
    expected: u64,
    inputs: Vec<ResourceInput>,
) -> anyhow::Result<ProviderView> {
    let snapshot = session.view().await?;
    let mut document = snapshot.document;
    ensure!(
        document.revision.0 == expected,
        "provider revision conflict"
    );
    let mut all = view_document(&document)?.resources;
    for input in inputs {
        all.retain(|r| r.name != input.name);
        all.push(input);
    }
    resources(&mut document, all, &snapshot.catalogue)?;
    session.configure(document).await?;
    view(local).await
}

pub fn enabled(local: &LocalNode, document: &mut Document, enabled: bool) -> anyhow::Result<()> {
    if enabled {
        document
            .listeners
            .entry("work".into())
            .or_insert(Listener::Iroh {
                port: crate::settings::load(&local.directory)?
                    .listen_port
                    .unwrap_or(0),
                chain: None,
                backends: vec![],
            });
        sync_listener(document);
    } else {
        document.listeners.remove("work");
    }
    Ok(())
}
