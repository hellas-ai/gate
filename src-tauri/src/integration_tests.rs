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
