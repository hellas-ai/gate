use crate::{
    control::{LocalNode, Session},
    dto::*,
};
use anyhow::{Context, ensure};
use hellas_sdk::daemon_config::{
    Backend, BackendKind, Credential, Document, Listener, Resource,
    ResourceKind as DaemonResourceKind, Upstream,
};
use std::collections::BTreeMap;

pub fn route_id(service: &str, method: &str) -> String {
    format!("{service}/{method}")
}
pub async fn view(local: &LocalNode) -> anyhow::Result<ProviderView> {
    let snapshot = local.session(false).await?.view().await?;
    view_document(&snapshot.document, snapshot.revision.0)
}
pub fn view_document(document: &Document, revision: u64) -> anyhow::Result<ProviderView> {
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
        .resources
        .iter()
        .map(|resource| input(document, resource))
        .collect::<anyhow::Result<_>>()?;
    Ok(ProviderView {
        revision,
        upstreams,
        resources,
    })
}
fn input(document: &Document, resource: &Resource) -> anyhow::Result<ResourceInput> {
    let BackendKind::Fetch {
        service, method, ..
    } = &document
        .backends
        .get(&resource.backend)
        .context("resource backend is missing")?
        .kind
    else {
        anyhow::bail!("resource backend is not Fetch")
    };
    let kind = match &resource.kind {
        DaemonResourceKind::Model { .. } => anyhow::bail!("Gate edits Fetch resources"),
        DaemonResourceKind::Responses { responses } => ResourceKind::OpenAi {
            model: responses.model.clone(),
            max_output_tokens: responses.max_output_tokens,
        },
        DaemonResourceKind::Https { https } => ResourceKind::Https {
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
        },
    };
    Ok(ResourceInput {
        name: resource.name.clone(),
        service: service.clone(),
        method: method.clone(),
        resource: kind,
    })
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
            put_key(
                document,
                &key.service,
                &key.method,
                DesiredKey {
                    name: key.key,
                    origins: key.origins,
                    paths: key.paths,
                    methods: key.methods,
                    header: key.header,
                    prefix: key.prefix,
                    secret: Some(key.secret),
                },
            )?;
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
    Ok(())
}
fn put(
    document: &mut Document,
    service: String,
    method: String,
    upstream: Upstream,
) -> anyhow::Result<()> {
    hellas_rpc::protocol::invocation::fetch::FetchRoutePolicy::sealed_route(&service, &method)?;
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
pub fn put_key(
    document: &mut Document,
    service: &str,
    method: &str,
    key: DesiredKey,
) -> anyhow::Result<()> {
    let BackendKind::Fetch {
        upstream: Upstream::Http { credentials, .. },
        ..
    } = &mut document
        .backends
        .get_mut(&route_id(service, method))
        .context("select an HTTP backend")?
        .kind
    else {
        anyhow::bail!("select an HTTP backend")
    };
    credentials.insert(
        key.name,
        Credential {
            trust_roots: hellas_rpc::http_fetch::HttpTrustRoots::WebPki,
            allowed_origins: key.origins,
            allowed_paths: key.paths,
            allowed_methods: key.methods,
            header_name: key.header,
            secret: key.secret.map(|secret| format!("{}{secret}", key.prefix)),
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
pub fn resources(document: &mut Document, inputs: Vec<ResourceInput>) -> anyhow::Result<()> {
    let mut names = std::collections::BTreeSet::new();
    let mut resources = vec![];
    for input in inputs {
        ensure!(names.insert(input.name.clone()), "duplicate resource name");
        let backend = route_id(&input.service, &input.method);
        ensure!(
            document.backends.contains_key(&backend),
            "resource backend is missing"
        );
        let kind = match input.resource {
            ResourceKind::OpenAi {
                model,
                max_output_tokens,
            } => DaemonResourceKind::Responses {
                responses: hellas_authority::model::grant::responses::ResponsesResource {
                    model,
                    max_output_tokens,
                    settings: Default::default(),
                },
            },
            ResourceKind::Https {
                origin,
                paths,
                methods,
                credential,
                max_response_bytes,
                tls_roots,
            } => DaemonResourceKind::Https {
                https: hellas_authority::model::grant::resource::HttpsResource {
                    origin,
                    paths,
                    methods,
                    credential,
                    max_response_bytes,
                    tls: hellas_rpc::http_fetch::HttpTls {
                        roots: if tls_roots.is_empty() {
                            hellas_rpc::http_fetch::HttpTrustRoots::WebPki
                        } else {
                            hellas_rpc::http_fetch::HttpTrustRoots::Certificates {
                                der_base64: tls_roots,
                            }
                        },
                        spki_sha256: vec![],
                    },
                    accounting: hellas_authority::model::http_usage::AccountingProfile::None,
                    max_output_tokens: 0,
                    model_policy: None,
                },
            },
        };
        let mut resource = document
            .resources
            .iter()
            .find(|r| r.name == input.name)
            .cloned()
            .unwrap_or_else(|| Resource::new(input.name, backend.clone(), kind.clone()));
        ensure!(
            resource.backend == backend,
            "a resource cannot move to another account"
        );
        resource.kind = kind;
        resources.push(resource);
    }
    document.resources = resources;
    Ok(())
}
pub async fn configure(
    session: &Session,
    expected: u64,
    edit: ProviderChange,
) -> anyhow::Result<ProviderView> {
    let view = session
        .edit(Some(expected), |document| change(document, edit))
        .await?;
    view_document(&view.document, view.revision.0)
}
pub async fn configure_resource(
    session: &Session,
    expected: u64,
    resource: ResourceInput,
) -> anyhow::Result<ProviderView> {
    let view = session
        .edit(Some(expected), |document| {
            let mut all = view_document(document, expected)?.resources;
            all.retain(|r| r.name != resource.name);
            all.push(resource);
            resources(document, all)
        })
        .await?;
    view_document(&view.document, view.revision.0)
}
pub fn enabled(document: &mut Document, enabled: bool) {
    if enabled {
        document
            .listeners
            .entry("work".into())
            .or_insert(Listener::Iroh {
                port: 0,
                chain: None,
                backends: vec![],
            });
        sync_listener(document);
    } else {
        document.listeners.remove("work");
    }
}
