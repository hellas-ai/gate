use crate::{clients::Clients, contacts, dto::*, state::AppState, trust, vault::Vault};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures::TryStreamExt;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Notify,
};

struct Upstream {
    url: String,
    root: String,
    calls: Arc<AtomicUsize>,
    hold: Arc<AtomicBool>,
    started: Arc<Notify>,
    release: Arc<Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl Upstream {
    async fn start() -> Self {
        let certificate = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let root = STANDARD.encode(certificate.cert.der());
        let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.signing_key.serialize_der())
                .into(),
        )
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("https://{}", listener.local_addr().unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let hold = Arc::new(AtomicBool::new(false));
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let (c, h, s, r) = (
            calls.clone(),
            hold.clone(),
            started.clone(),
            release.clone(),
        );
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let (a, c, h, s, r) =
                    (acceptor.clone(), c.clone(), h.clone(), s.clone(), r.clone());
                tokio::spawn(async move {
                    let Ok(mut stream) = a.accept(stream).await else {
                        return;
                    };
                    let mut bytes = Vec::new();
                    while !bytes.ends_with(b"\r\n\r\n") {
                        let Ok(byte) = stream.read_u8().await else {
                            return;
                        };
                        bytes.push(byte);
                        if bytes.len() > 8192 {
                            return;
                        }
                    }
                    c.fetch_add(1, Ordering::SeqCst);
                    s.notify_one();
                    if h.load(Ordering::SeqCst) {
                        r.notified().await;
                    }
                    let _=stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello").await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self {
            url,
            root,
            calls,
            hold,
            started,
            release,
            task,
        }
    }
}
impl Drop for Upstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn execute(
    clients: &Clients,
    local: &crate::control::LocalNode,
    request: &RunRequest,
) -> anyhow::Result<Vec<hellas_rpc::output::OutputEvent>> {
    Ok(clients.fetch(local, request).await?.try_collect().await?)
}

#[tokio::test(flavor = "multi_thread")]
async fn real_authorized_fetch_recovers_issuance_preserves_live_composition_quota_and_drain() {
    let upstream = Upstream::start().await;
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let sa = tempfile::tempdir().unwrap();
    let sb = tempfile::tempdir().unwrap();
    let av = Vault::fixture(sa.path());
    let alice = Arc::new(AppState::with_vault(a.path(), av.clone()).await.unwrap());
    let bob = AppState::with_vault(b.path(), Vault::fixture(sb.path()))
        .await
        .unwrap();
    alice.initialize_software("Alice".into()).await.unwrap();
    let bs = bob.initialize_software("Bob".into()).await.unwrap();
    alice.bind_provider(true).await.unwrap();
    alice
        .configure_provider(
            0,
            ProviderChange::Endpoint {
                service: "web".into(),
                method: "get".into(),
                hosts: vec!["127.0.0.1".into()],
                allow_private_addresses: true,
            },
        )
        .await
        .unwrap();
    alice.set_provider(true).await.unwrap();
    alice
        .configure_resource(
            1,
            ResourceInput {
                name: "page".into(),
                service: "web".into(),
                method: "get".into(),
                resource: ResourceKind::Https {
                    origin: upstream.url.clone(),
                    paths: vec!["/".into()],
                    methods: vec!["GET".into()],
                    credential: None,
                    max_response_bytes: 4096,
                    tls_roots: vec![upstream.root.clone()],
                },
            },
        )
        .await
        .unwrap();
    alice
        .configure_provider(
            2,
            ProviderChange::Endpoint {
                service: "openai".into(),
                method: "responses".into(),
                hosts: vec!["api.openai.com".into()],
                allow_private_addresses: false,
            },
        )
        .await
        .unwrap();
    for (revision, alias) in [(3, "first"), (4, "second")] {
        alice
            .configure_provider(
                revision,
                ProviderChange::AddKey {
                    key: UpstreamKeyInput {
                        service: "openai".into(),
                        method: "responses".into(),
                        key: alias.into(),
                        origins: vec!["https://api.openai.com".into()],
                        paths: vec!["/v1/responses".into()],
                        methods: vec!["POST".into()],
                        header: "authorization".into(),
                        prefix: "Bearer ".into(),
                        secret: format!("fixture-{alias}"),
                    },
                },
            )
            .await
            .unwrap();
    }
    alice
        .configure_resource(
            5,
            ResourceInput {
                name: "responses".into(),
                service: "openai".into(),
                method: "responses".into(),
                resource: ResourceKind::OpenAi {
                    model: "fixture-model".into(),
                    max_output_tokens: 64,
                    credential: "first".into(),
                },
            },
        )
        .await
        .unwrap();
    let contact = contacts::add(
        a.path(),
        ContactInput {
            card: bs.identity.contact_card,
            holder: bs.identity.node_key.clone(),
            confirmed_holder: true,
            label: "Bob".into(),
        },
    )
    .unwrap();
    let local = alice.local().await.unwrap();
    let recipient = bob.local().await.unwrap();
    let share_input = ShareInput {
        request_id: hex::encode([45; 16]),
        contact: contact.id,
        holder: bs.identity.node_key,
        allowances: vec![AllowanceInput {
            resource: "page".into(),
            requests_per_day: 2,
            max_in_flight: 1,
        }],
        period: crate::period::Period::Week,
        label: "page".into(),
    };
    let key_input = GatewayKeyInput {
        request_id: hex::encode([46; 16]),
        label: "crash retry".into(),
        period: crate::period::Period::Week,
    };
    crate::vault::publication_fault::fail_next_write(
        a.path()
            .join("shares")
            .join(format!("{}.json", share_input.request_id)),
    );
    let error = contacts::share(a.path(), &local, share_input.clone())
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("injected metadata publication failure")
    );
    let share_prepared: crate::credentials::PreparedIssue = crate::vault::read(
        &a.path()
            .join("prepared-issues")
            .join(format!("{}.json", share_input.request_id)),
    )
    .unwrap()
    .unwrap();
    let share_secret = share_prepared.secret(&av).await.unwrap();
    crate::vault::publication_fault::fail_next_write(
        a.path()
            .join("gateway-keys")
            .join(format!("{}.json", key_input.request_id)),
    );
    let error = alice
        .issue_gateway_key(key_input.clone())
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("injected metadata publication failure")
    );
    let key_prepared: crate::credentials::PreparedIssue = crate::vault::read(
        &a.path()
            .join("prepared-gateway-keys")
            .join(format!("{}.json", key_input.request_id)),
    )
    .unwrap()
    .unwrap();
    let key_secret = key_prepared.secret(&av).await.unwrap();
    assert!(local.coordinator.authenticate(&key_secret).await.is_err());
    assert!(local.coordinator.authenticate(&share_secret).await.is_err());
    alice.shutdown().await.unwrap();
    drop(local);
    drop(alice);
    let alice = Arc::new(AppState::with_vault(a.path(), av.clone()).await.unwrap());
    alice.set_provider(true).await.unwrap();
    let local = alice.local().await.unwrap();
    // Reopen and retry under the same stable intent, after now/not_before have changed.
    let shared = contacts::share(a.path(), &local, share_input.clone())
        .await
        .unwrap();
    assert_eq!(
        shared.credential,
        share_prepared.credential.id().to_string()
    );
    let saved: contacts::Shared = crate::vault::read(
        &a.path()
            .join("shares")
            .join(format!("{}.json", share_input.request_id)),
    )
    .unwrap()
    .unwrap();
    assert_eq!(saved.issue.credential, share_prepared.credential);
    assert_eq!(
        saved.issue.secret(&av).await.unwrap().expose_secret(),
        share_secret.expose_secret()
    );
    let (_, bearer) = alice.issue_gateway_key(key_input).await.unwrap();
    assert_eq!(
        bearer,
        format!(
            "{}{}",
            key_secret.id(),
            hex::encode(key_secret.expose_secret())
        )
    );
    local.coordinator.authenticate(&key_secret).await.unwrap();
    // Changed intent under the exact same ID must still be refused.
    let mut changed = share_input;
    changed.label = "different intent".into();
    assert!(contacts::share(a.path(), &local, changed).await.is_err());
    let bundle = contacts::export(a.path(), &local, &shared.id)
        .await
        .unwrap();
    let imported = contacts::import(b.path(), &recipient, &bundle)
        .await
        .unwrap();
    let policy = local.composition.policy("page").unwrap();
    let request = RunRequest {
        kind: RunKind::Fetch,
        input: serde_json::to_string(&hellas_rpc::http_fetch::HttpFetchRequest {
            url: format!("{}/", upstream.url),
            method: "GET".into(),
            headers: vec![],
            body_base64: String::new(),
            tls: hellas_rpc::http_fetch::HttpTls {
                roots: hellas_rpc::http_fetch::HttpTrustRoots::Certificates {
                    der_base64: vec![upstream.root.clone()],
                },
                spki_sha256: vec![],
            },
            credential: None,
            max_response_bytes: 4096,
        })
        .unwrap(),
        client: WorkClientConfig {
            target: WorkTarget::Authorized {
                access_id: imported.id.clone(),
                allowance: shared.allowances[0].clone(),
                resource: "page".into(),
            },
            trust_policy: "alice".into(),
        },
    };
    assert!(
        execute(&recipient.clients, &recipient, &request)
            .await
            .is_err()
    );
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 0);
    let approval = |revision, genesis| TrustInput {
        id: "alice".into(),
        expected_revision: revision,
        genesis,
        apple: false,
        environments: vec![policy.work.allowed_environment().to_string()],
        app_id: String::new(),
        cd_hashes: vec![],
        reason: "Independent local fixture pin".into(),
    };
    trust::approve(b.path(), approval(0, hex::encode([0; 32]))).unwrap();
    assert!(
        execute(&recipient.clients, &recipient, &request)
            .await
            .is_err()
    );
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 0);
    trust::approve(
        b.path(),
        approval(
            1,
            local
                .coordinator
                .provider()
                .await
                .unwrap()
                .unwrap()
                .content_id()
                .to_string(),
        ),
    )
    .unwrap();
    let events = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        execute(&recipient.clients, &recipient, &request),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, hellas_rpc::output::OutputEvent::Finished { .. }))
    );
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);
    // Consume the first request's notification before holding the second.
    upstream.started.notified().await;
    upstream.hold.store(true, Ordering::SeqCst);
    let (r, q) = (recipient.clone(), request.clone());
    let inflight = tokio::spawn(async move { execute(&r.clients, &r, &q).await });
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        upstream.started.notified(),
    )
    .await
    .unwrap();
    let before = std::fs::read(a.path().join("provider-configuration.json")).unwrap();
    let authority_before = local
        .session(false)
        .await
        .unwrap()
        .status()
        .await
        .unwrap()
        .authority_revision;
    for resource in [
        ResourceKind::Https {
            origin: "https://api.openai.com".into(),
            paths: vec!["/v1/responses".into()],
            methods: vec!["POST".into()],
            credential: Some("first".into()),
            max_response_bytes: 4096,
            tls_roots: vec![],
        },
        ResourceKind::OpenAi {
            model: "other-model".into(),
            max_output_tokens: 64,
            credential: "second".into(),
        },
    ] {
        // A late start-time validation would block while draining this held request.
        let failure = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            alice.configure_resource(
                6,
                ResourceInput {
                    name: "incompatible".into(),
                    service: "openai".into(),
                    method: "responses".into(),
                    resource,
                },
            ),
        )
        .await
        .expect("invalid catalogue must be refused without stopping/draining")
        .unwrap_err();
        assert!(
            failure
                .to_string()
                .contains("one endpoint cannot mix resource drivers")
                || failure.to_string().contains("same credential alias")
        );
        assert_eq!(
            std::fs::read(a.path().join("provider-configuration.json")).unwrap(),
            before
        );
        assert_eq!(alice.provider_view().await.unwrap().revision, 6);
        assert_eq!(alice.status().await.unwrap().provider.state, "running");
        assert!(!inflight.is_finished());
        assert_eq!(
            local
                .session(false)
                .await
                .unwrap()
                .status()
                .await
                .unwrap()
                .authority_revision,
            authority_before
        );
    }
    assert!(
        alice
            .configure_provider(
                0,
                ProviderChange::RemoveEndpoint {
                    service: "web".into(),
                    method: "get".into()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(alice.status().await.unwrap().provider.state, "running");
    let quitting = alice.clone();
    let shutdown = tokio::spawn(async move { quitting.shutdown().await });
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    assert!(
        !shutdown.is_finished(),
        "shutdown must wait for the accepted upstream response"
    );
    upstream.release.notify_one();
    let events = tokio::time::timeout(std::time::Duration::from_secs(20), inflight)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, hellas_rpc::output::OutputEvent::Finished { .. }))
    );
    assert!(shutdown.await.unwrap().unwrap());
    assert!(!local.socket.exists());
    assert!(alice.set_provider(true).await.is_err());
    recipient.clients.reset().await.unwrap();
    drop(local);
    drop(alice);
    let alice = AppState::with_vault(a.path(), av).await.unwrap();
    alice.set_provider(true).await.unwrap();
    assert!(
        execute(&recipient.clients, &recipient, &request)
            .await
            .is_err()
    );
    assert_eq!(
        upstream.calls.load(Ordering::SeqCst),
        2,
        "quota refusal after restart must happen before upstream"
    );
    alice.shutdown().await.unwrap();
    bob.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn gateway_admits_only_current_named_credentials_and_zdr_does_not_archive() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let sa = tempfile::tempdir().unwrap();
    let sb = tempfile::tempdir().unwrap();
    let alice = AppState::with_vault(a.path(), Vault::fixture(sa.path()))
        .await
        .unwrap();
    let bob = AppState::with_vault(b.path(), Vault::fixture(sb.path()))
        .await
        .unwrap();
    alice.initialize_software("Alice".into()).await.unwrap();
    let bs = bob.initialize_software("Bob".into()).await.unwrap();
    alice.bind_provider(true).await.unwrap();
    alice
        .configure_provider(
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
    alice
        .configure_provider(
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
                    secret: "fixture-never-used-upstream-key".into(),
                },
            },
        )
        .await
        .unwrap();
    alice.set_provider(true).await.unwrap();
    alice
        .configure_resource(
            2,
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
    let contact = contacts::add(
        a.path(),
        ContactInput {
            card: bs.identity.contact_card,
            holder: bs.identity.node_key.clone(),
            confirmed_holder: true,
            label: "Bob".into(),
        },
    )
    .unwrap();
    let local = alice.local().await.unwrap();
    let recipient = bob.local().await.unwrap();
    let shared = contacts::share(
        a.path(),
        &local,
        ShareInput {
            request_id: hex::encode([50; 16]),
            contact: contact.id,
            holder: bs.identity.node_key,
            allowances: vec![AllowanceInput {
                resource: "responses".into(),
                requests_per_day: 2,
                max_in_flight: 1,
            }],
            period: crate::period::Period::Week,
            label: "responses".into(),
        },
    )
    .await
    .unwrap();
    let bundle = contacts::export(a.path(), &local, &shared.id)
        .await
        .unwrap();
    let access = contacts::import(b.path(), &recipient, &bundle)
        .await
        .unwrap();
    let policy = local.composition.policy("responses").unwrap();
    trust::approve(
        b.path(),
        TrustInput {
            id: "alice".into(),
            expected_revision: 0,
            genesis: local
                .coordinator
                .provider()
                .await
                .unwrap()
                .unwrap()
                .content_id()
                .to_string(),
            apple: false,
            environments: vec![policy.work.allowed_environment().to_string()],
            app_id: String::new(),
            cd_hashes: vec![],
            reason: "Independent local fixture pin".into(),
        },
    )
    .unwrap();
    let config = GatewayConfig {
        client: WorkClientConfig {
            target: WorkTarget::Authorized {
                access_id: access.id,
                allowance: shared.allowances[0].clone(),
                resource: "responses".into(),
            },
            trust_policy: "alice".into(),
        },
        http_routes: vec![],
        port: Some(0),
        zdr: true,
    };
    bob.set_gateway(true, Some(config)).await.unwrap();
    let address = bob.gateway_address().await.unwrap();
    let input = GatewayKeyInput {
        request_id: hex::encode([51; 16]),
        label: "tool one".into(),
        period: crate::period::Period::Week,
    };
    let (first, bearer) = bob.issue_gateway_key(input.clone()).await.unwrap();
    let (retry, again) = bob.issue_gateway_key(input).await.unwrap();
    assert_eq!(first.id, retry.id);
    assert_eq!(bearer, again);
    let (_, second) = bob
        .issue_gateway_key(GatewayKeyInput {
            request_id: hex::encode([52; 16]),
            label: "tool two".into(),
            period: crate::period::Period::Week,
        })
        .await
        .unwrap();
    let http = reqwest::Client::new();
    let url = format!("{address}/v1/responses");
    assert_eq!(
        http.post(&url)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        http.post(&url)
            .bearer_auth("wrong")
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    // Invalid request content is rejected by the real Responses parser after HTTP admission.
    assert_ne!(
        http.post(&url)
            .bearer_auth(&bearer)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    bob.revoke_gateway_key(&first.id).await.unwrap();
    assert_eq!(
        http.post(&url)
            .bearer_auth(&bearer)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_ne!(
        http.post(&url)
            .bearer_auth(&second)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert!(
        bob.gateway_keys()
            .await
            .unwrap()
            .iter()
            .any(|key| key.id == first.id && key.revoked)
    );
    bob.set_gateway(false, None).await.unwrap();
    assert!(http.post(&url).send().await.is_err());
    assert!(!b.path().join("gateway-archive").exists());
    let metadata = std::fs::read_to_string(
        b.path()
            .join("gateway-keys")
            .join(format!("{}.json", first.id)),
    )
    .unwrap();
    assert!(!metadata.contains(&bearer));
    bob.shutdown().await.unwrap();
    alice.shutdown().await.unwrap();
}

// Real provider, contact handoff, SDK-issued grant and approved trust for client regressions.
async fn grant_https(
    provider: &AppState,
    recipient: &AppState,
    upstream: &Upstream,
    label: &str,
    request_byte: u8,
) -> (RunRequest, ShareView) {
    provider.bind_provider(true).await.unwrap();
    provider
        .configure_provider(
            0,
            ProviderChange::Endpoint {
                service: "web".into(),
                method: "get".into(),
                hosts: vec!["127.0.0.1".into()],
                allow_private_addresses: true,
            },
        )
        .await
        .unwrap();
    provider.set_provider(true).await.unwrap();
    provider
        .configure_resource(
            1,
            ResourceInput {
                name: "page".into(),
                service: "web".into(),
                method: "get".into(),
                resource: ResourceKind::Https {
                    origin: upstream.url.clone(),
                    paths: vec!["/".into()],
                    methods: vec!["GET".into()],
                    credential: None,
                    max_response_bytes: 4096,
                    tls_roots: vec![upstream.root.clone()],
                },
            },
        )
        .await
        .unwrap();
    let recipient_status = recipient.status().await.unwrap();
    let contact = provider
        .add_contact(ContactInput {
            card: recipient_status.identity.contact_card,
            holder: recipient_status.identity.node_key.clone(),
            confirmed_holder: true,
            label: label.into(),
        })
        .await
        .unwrap();
    let shared = provider
        .share(ShareInput {
            request_id: hex::encode([request_byte; 16]),
            contact: contact.id,
            holder: recipient_status.identity.node_key,
            allowances: vec![AllowanceInput {
                resource: "page".into(),
                requests_per_day: 3,
                max_in_flight: 1,
            }],
            period: crate::period::Period::Week,
            label: label.into(),
        })
        .await
        .unwrap();
    let access = recipient
        .import_access(&provider.export_access(&shared.id).await.unwrap())
        .await
        .unwrap();
    let local = provider.local().await.unwrap();
    trust::approve(
        &recipient.directory,
        TrustInput {
            id: label.into(),
            expected_revision: 0,
            genesis: local
                .coordinator
                .provider()
                .await
                .unwrap()
                .unwrap()
                .content_id()
                .to_string(),
            apple: false,
            environments: vec![
                local
                    .composition
                    .policy("page")
                    .unwrap()
                    .work
                    .allowed_environment()
                    .to_string(),
            ],
            app_id: String::new(),
            cd_hashes: vec![],
            reason: "Independent fixture provider pin".into(),
        },
    )
    .unwrap();
    (
        RunRequest {
            kind: RunKind::Fetch,
            input: serde_json::to_string(&hellas_rpc::http_fetch::HttpFetchRequest {
                url: format!("{}/", upstream.url),
                method: "GET".into(),
                headers: vec![],
                body_base64: String::new(),
                tls: hellas_rpc::http_fetch::HttpTls {
                    roots: hellas_rpc::http_fetch::HttpTrustRoots::Certificates {
                        der_base64: vec![upstream.root.clone()],
                    },
                    spki_sha256: vec![],
                },
                credential: None,
                max_response_bytes: 4096,
            })
            .unwrap(),
            client: WorkClientConfig {
                target: WorkTarget::Authorized {
                    access_id: access.id,
                    allowance: shared.allowances[0].clone(),
                    resource: "page".into(),
                },
                trust_policy: label.into(),
            },
        },
        shared,
    )
}
fn finished(events: &[hellas_rpc::output::OutputEvent]) -> bool {
    events
        .iter()
        .any(|event| matches!(event, hellas_rpc::output::OutputEvent::Finished { .. }))
}

#[tokio::test(flavor = "multi_thread")]
async fn real_authorized_trust_revision_and_policy_switch_reuse_one_journal_and_quota() {
    use hellas_gateway::WorkExecutionBackend as _;
    let upstream = Upstream::start().await;
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let sa = tempfile::tempdir().unwrap();
    let sb = tempfile::tempdir().unwrap();
    let alice = AppState::with_vault(a.path(), Vault::fixture(sa.path()))
        .await
        .unwrap();
    let bob = AppState::with_vault(b.path(), Vault::fixture(sb.path()))
        .await
        .unwrap();
    alice.initialize_software("Alice".into()).await.unwrap();
    bob.initialize_software("Bob".into()).await.unwrap();
    let (mut request, shared) = grant_https(&alice, &bob, &upstream, "alice", 70).await;
    let recipient = bob.local().await.unwrap();
    let original = recipient
        .clients
        .target(&recipient, &request.client)
        .await
        .unwrap();
    assert!(finished(
        &execute(&recipient.clients, &recipient, &request)
            .await
            .unwrap()
    ));
    let authority = alice.local().await.unwrap().identity.public.authority;
    let root = b
        .path()
        .join("work-channels")
        .join(authority.to_string())
        .join(&shared.allowances[0]);
    let channels = || {
        std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<std::collections::BTreeSet<_>>()
    };
    let before = channels();
    assert_eq!(before.len(), 1);
    let approval = trust::list(b.path()).unwrap().pop().unwrap();
    let revise = |id, revision| TrustInput {
        id,
        expected_revision: revision,
        genesis: approval.genesis.clone(),
        apple: false,
        environments: approval.environments.clone(),
        app_id: String::new(),
        cd_hashes: vec![],
        reason: "Independently rechecked same pins".into(),
    };
    trust::approve(b.path(), revise("alice".into(), 1)).unwrap();
    assert!(finished(
        &execute(&recipient.clients, &recipient, &request)
            .await
            .unwrap()
    ));
    assert!(
        original
            .backend
            .fetch(hellas_gateway::WorkFetchRequest {
                provider: original.provider,
                service: original.service.clone(),
                method: original.method.clone(),
                body: request.input.as_bytes().to_vec()
            })
            .is_err(),
        "a retained old target must not retain an open journal or admit new work"
    );
    trust::approve(b.path(), revise("other-policy".into(), 0)).unwrap();
    request.client.trust_policy = "other-policy".into();
    assert!(finished(
        &execute(&recipient.clients, &recipient, &request)
            .await
            .unwrap()
    ));
    assert_eq!(channels(), before);
    assert!(
        execute(&recipient.clients, &recipient, &request)
            .await
            .is_err()
    );
    assert_eq!(
        upstream.calls.load(Ordering::SeqCst),
        3,
        "trust refresh must preserve the same quota history"
    );
    bob.shutdown().await.unwrap();
    alice.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn independent_authorities_with_the_same_credential_id_remain_usable_after_import_and_reopen()
{
    use hellas_auth::{authority::*, hash_tuple};
    use hellas_sdk::access_bundle::AccessBundle;
    let upstream = Upstream::start().await;
    let a = tempfile::tempdir().unwrap();
    let c = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let sa = tempfile::tempdir().unwrap();
    let sc = tempfile::tempdir().unwrap();
    let sb = tempfile::tempdir().unwrap();
    let alice = AppState::with_vault(a.path(), Vault::fixture(sa.path()))
        .await
        .unwrap();
    let carol = AppState::with_vault(c.path(), Vault::fixture(sc.path()))
        .await
        .unwrap();
    let vault = Vault::fixture(sb.path());
    let bob = AppState::with_vault(b.path(), vault.clone()).await.unwrap();
    alice.initialize_software("Alice".into()).await.unwrap();
    carol.initialize_software("Carol".into()).await.unwrap();
    bob.initialize_software("Bob".into()).await.unwrap();
    // Deliberately collide with local application custody too.
    let (_, local_bearer) = bob
        .issue_gateway_key(GatewayKeyInput {
            request_id: hex::encode([88; 16]),
            label: "retained local key".into(),
            period: crate::period::Period::Week,
        })
        .await
        .unwrap();
    let collision: CredentialId = local_bearer[..32].parse().unwrap();
    let local_key = crate::credentials::PreparedIssue::key(collision);
    let before = vault.required(&local_key).await.unwrap();
    let mut requests = Vec::new();
    let mut imports = Vec::new();
    for (index, (provider, label)) in [(&alice, "alice"), (&carol, "carol")]
        .into_iter()
        .enumerate()
    {
        let (mut request, shared) =
            grant_https(provider, &bob, &upstream, label, 80 + index as u8).await;
        let local = provider.local().await.unwrap();
        let saved: contacts::Shared = crate::vault::read(
            &provider
                .directory
                .join("shares")
                .join(format!("{}.json", shared.id)),
        )
        .unwrap()
        .unwrap();
        let mut bundle = AccessBundle::decode(
            &STANDARD
                .decode(provider.export_access(&shared.id).await.unwrap())
                .unwrap(),
            &bob.local().await.unwrap().identity.public.owner,
            &saved.holder,
        )
        .unwrap();
        let secret = CredentialSecret::from_parts(collision, [90 + index as u8; 32]);
        // The public trusted-record serializer lets this fixture choose an ID.
        // The real authority must still admit its scope, generation and verifier.
        let generation = local
            .session(false)
            .await
            .unwrap()
            .status()
            .await
            .unwrap()
            .generation;
        let mut record = serde_json::to_value(&saved.issue.credential).unwrap();
        record["id"] = serde_json::to_value(collision).unwrap();
        record["verifier"] = serde_json::to_value(hash_tuple(
            "hellas.auth.bearer.v1",
            &[
                bundle.authority.0.as_bytes(),
                &generation.to_be_bytes(),
                &collision.0,
                secret.expose_secret(),
            ],
        ))
        .unwrap();
        let credential: Credential = serde_json::from_value(record).unwrap();
        local
            .session(true)
            .await
            .unwrap()
            .issue(&credential)
            .await
            .unwrap();
        bundle.credential = secret;
        let encoded = STANDARD.encode(bundle.encode().unwrap());
        let imported = bob.import_access(&encoded).await.unwrap();
        assert_eq!(bob.import_access(&encoded).await.unwrap().id, imported.id);
        if let WorkTarget::Authorized { access_id, .. } = &mut request.client.target {
            *access_id = imported.id.clone();
        }
        bundle.credential = CredentialSecret::from_parts(collision, [99; 32]);
        assert!(
            bob.import_access(&STANDARD.encode(bundle.encode().unwrap()))
                .await
                .is_err(),
            "same authority/id must not replace a persisted secret"
        );
        imports.push(imported.id);
        requests.push(request);
    }
    assert_ne!(imports[0], imports[1]);
    assert_eq!(
        vault.required(&local_key).await.unwrap().as_slice(),
        before.as_slice()
    );
    // Discard the ordinary random-ID grants; exercise the two colliding imports after reopen.
    bob.shutdown().await.unwrap();
    drop(bob);
    let bob = AppState::with_vault(b.path(), vault).await.unwrap();
    assert!(bob.status().await.unwrap().identity.initialized);
    let recipient = bob.local().await.unwrap();
    for request in requests {
        assert!(finished(
            &execute(&recipient.clients, &recipient, &request)
                .await
                .unwrap()
        ));
    }
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 2);
    let ids = bob
        .accesses()
        .await
        .unwrap()
        .into_iter()
        .map(|entry| entry.id)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(imports.iter().all(|id| ids.contains(id)));
    bob.shutdown().await.unwrap();
    alice.shutdown().await.unwrap();
    carol.shutdown().await.unwrap();
}

async fn paid_policy_fixture(directory: &std::path::Path) -> PaidPolicyInput {
    use commonware_codec::DecodeExt as _;
    use commonware_cryptography::Digestible as _;
    use commonware_runtime::Runner as _;
    let mut genesis: hellas_genesis::Genesis =
        serde_json::from_str(hellas_genesis::HELLAS_DEVNET_1_JSON).unwrap();
    genesis.network_id = "gate-paid-cache-test".into();
    genesis.allocations.clear();
    let leader = genesis
        .validators
        .iter()
        .map(|validator| {
            hellas_chain::domain::PublicKey::decode(
                hex::decode(&validator.public_key).unwrap().as_slice(),
            )
            .unwrap()
        })
        .min()
        .unwrap();
    let network = hellas_sdk::kernel::NetworkId::new(&genesis.network_id).unwrap();
    let genesis_document = hellas_chain::genesis_digest(&genesis);
    let directory = directory.join("genesis-probe");
    let digest = tokio::task::spawn_blocking(move || {
        let runtime = commonware_runtime::tokio::Runner::new(
            commonware_runtime::tokio::Config::new().with_storage_directory(directory),
        );
        runtime.start(move |context| async move {
            let application = hellas_chain::Application::new(
                context,
                network,
                genesis_document,
                leader,
                vec![],
                "probe",
                Default::default(),
            )
            .await;
            application.genesis_block().digest()
        })
    })
    .await
    .unwrap();
    PaidPolicyInput {
        chain:ChainInput {network:genesis.network_id,genesis_digest:hellas_auth::Digest::from(digest.0).to_string(),threshold_identity:"97f1d3a73197d7942695638c4fa9ac0fc3688c4f9774b905a14e3a3f171bac586c55e83ff97a1aeffb3af00adb22c6bb".into(),validators:(1..=6).map(|port|format!("http://127.0.0.1:{}",10000+port)).collect(),committee:genesis.validators.into_iter().map(|v|GenesisValidatorInput {public_key:v.public_key,label:v.label}).collect(),allocations:vec![]},
        service:"openai".into(),method:"responses".into(),presentation:FetchPresentation::Responses,policy_salt:hex::encode([9;32]),compute_credit_limit:40,delivery_credit_limit:40,fixed_price:10,dispatch_blocks:1,delivery_blocks:1,oracle_blocks:1,value:1000,reserve:200,fee_base:0,fee_slot:0,fee_proof:0,fee_lifetime:0,min_omit_response_blocks:10,poll_ms:25,max_observation_age_ms:5000,
    }
}
#[tokio::test(flavor = "multi_thread")]
async fn saving_paid_pool_rebuilds_real_sdk_backend_with_current_settings() {
    use hellas_gateway::WorkExecutionBackend as _;
    let directory = tempfile::tempdir().unwrap();
    let secrets = tempfile::tempdir().unwrap();
    let app = AppState::with_vault(directory.path(), Vault::fixture(secrets.path()))
        .await
        .unwrap();
    app.initialize_software("Paid client".into()).await.unwrap();
    let policy = paid_policy_fixture(directory.path()).await;
    trust::approve(
        directory.path(),
        TrustInput {
            id: "paid-provider".into(),
            expected_revision: 0,
            genesis: hex::encode([4; 32]),
            apple: false,
            environments: vec![
                crate::paid::work_policy(policy.presentation, &policy.service, &policy.method)
                    .unwrap()
                    .allowed_environment()
                    .to_string(),
            ],
            app_id: String::new(),
            cd_hashes: vec![],
            reason: "Independent fixture pin".into(),
        },
    )
    .unwrap();
    let provider = hellas_sdk::iroh::SecretKey::from_bytes(&[3; 32])
        .public()
        .to_string();
    let mut pool = PaidPoolInput {
        id: "same-pool".into(),
        providers: vec![PaidProviderInput {
            provider: provider.clone(),
            addresses: vec![],
            trust_policy: "paid-provider".into(),
            bond: hex::encode([12; 32]),
            coins: vec![hex::encode([1; 32])],
            omission_bond: 601,
            acceptance_blocks: 16,
            terminal_blocks: 64,
            payment_blocks: 32,
            timeout_seconds: 1,
            policy,
        }],
        max_pending_requests: 4,
    };
    app.save_paid_pool(pool.clone()).await.unwrap();
    let local = app.local().await.unwrap();
    let input = WorkClientConfig {
        target: WorkTarget::Paid {
            pool_id: pool.id.clone(),
            provider,
            service: "openai".into(),
            method: "responses".into(),
        },
        trust_policy: "paid-provider".into(),
    };
    let original = local.clients.target(&local, &input).await.unwrap();
    assert_eq!(
        original.backend.timeout(),
        std::time::Duration::from_secs(1)
    );
    let approved = trust::list(directory.path()).unwrap().pop().unwrap();
    trust::approve(
        directory.path(),
        TrustInput {
            id: "alternate".into(),
            expected_revision: 0,
            genesis: approved.genesis,
            apple: false,
            environments: approved.environments,
            app_id: String::new(),
            cd_hashes: vec![],
            reason: "Independent alternate policy".into(),
        },
    )
    .unwrap();
    let mut invalid = input.clone();
    invalid.trust_policy = "alternate".into();
    let failure = local.clients.target(&local, &invalid).await.err().unwrap();
    assert!(failure.to_string().contains("trust policy must match"));
    let retry = local.clients.target(&local, &input).await.unwrap();
    assert!(
        Arc::ptr_eq(&original, &retry),
        "invalid selection must leave the original target cached and usable"
    );
    assert_eq!(
        original.backend.timeout(),
        std::time::Duration::from_secs(1)
    );
    drop(
        original
            .backend
            .fetch(hellas_gateway::WorkFetchRequest {
                provider: original.provider,
                service: original.service.clone(),
                method: original.method.clone(),
                body: br#"{"model":"fixture","input":"hello","max_output_tokens":8}"#.to_vec(),
            })
            .expect("the real paid backend still admits a valid request"),
    );
    pool.providers[0].timeout_seconds = 2;
    pool.providers[0].coins = vec![hex::encode([2; 32])];
    pool.providers[0].policy.fixed_price = 11;
    app.save_paid_pool(pool).await.unwrap();
    let current = local.clients.target(&local, &input).await.unwrap();
    assert!(!Arc::ptr_eq(&original, &current));
    assert_eq!(current.backend.timeout(), std::time::Duration::from_secs(2));
    assert!(
        original
            .backend
            .fetch(hellas_gateway::WorkFetchRequest {
                provider: original.provider,
                service: original.service.clone(),
                method: original.method.clone(),
                body: b"{}".to_vec()
            })
            .is_err()
    );
    // Pool startup/reload creates no funded channel and preserves the same journal root.
    assert!(!directory.path().join("paid-channels/same-pool").exists());
    app.shutdown().await.unwrap();
}
