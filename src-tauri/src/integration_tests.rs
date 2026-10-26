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
    fail: Arc<AtomicBool>,
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
        let fail = Arc::new(AtomicBool::new(false));
        let broken = fail.clone();
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
                let broken = broken.clone();
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
                    if broken.load(Ordering::SeqCst) {
                        let _ = stream.shutdown().await;
                        return;
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
            fail,
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
    let bob_vault = Vault::fixture(sb.path());
    let mut bob = AppState::with_vault(b.path(), bob_vault.clone())
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
    drop(recipient);
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
    bob.set_gateway(true, Some(config.clone())).await.unwrap();
    let input = GatewayKeyInput {
        request_id: hex::encode([0xab; 16]).to_uppercase(),
        label: "tool one".into(),
        period: crate::period::Period::Week,
    };
    let (first, bearer) = bob.issue_gateway_key(input.clone()).await.unwrap();
    assert_eq!(first.id, hex::encode([0xab; 16]));
    let mut retry_input = input;
    retry_input.request_id = first.id.clone();
    let (retry, again) = bob.issue_gateway_key(retry_input).await.unwrap();
    assert_eq!(first.id, retry.id);
    assert_eq!(bearer, again);
    let (second_key, second) = bob
        .issue_gateway_key(GatewayKeyInput {
            request_id: hex::encode([52; 16]),
            label: "tool two".into(),
            period: crate::period::Period::Week,
        })
        .await
        .unwrap();
    let pending_input = GatewayKeyInput {
        request_id: hex::encode([0xef; 16]),
        label: "pending tool".into(),
        period: crate::period::Period::Week,
    };
    let prepared = {
        let local = bob.local().await.unwrap();
        let limited = restricted_session(&local, &[hellas_sdk::work::node::HOST_STATUS]).await;
        assert!(
            crate::gateway::issue_session(&local, pending_input.clone(), &limited)
                .await
                .is_err()
        );
        let prepared: crate::credentials::PreparedIssue = crate::vault::read(
            &b.path()
                .join("prepared-gateway-keys")
                .join(format!("{}.json", pending_input.request_id)),
        )
        .unwrap()
        .unwrap();
        assert!(
            local
                .coordinator
                .authenticate(&prepared.secret(&bob_vault).await.unwrap())
                .await
                .is_err()
        );
        prepared
    };
    let original_secret = prepared.secret(&bob_vault).await.unwrap();
    let pending_bearer = format!(
        "{}{}",
        original_secret.id(),
        hex::encode(original_secret.expose_secret())
    );
    bob.shutdown().await.unwrap();
    bob = AppState::with_vault(b.path(), bob_vault.clone())
        .await
        .unwrap();
    let listed = bob.gateway_keys().await.unwrap();
    assert!(
        listed
            .iter()
            .any(|key| key.id == first.id && key.state == GatewayKeyState::Active)
    );
    assert!(
        listed
            .iter()
            .any(|key| key.id == pending_input.request_id && key.state == GatewayKeyState::Pending)
    );
    bob.set_gateway(true, Some(config)).await.unwrap();
    let address = bob.gateway_address().await.unwrap();
    let http = reqwest::Client::new();
    let url = format!("{address}/v1/responses");
    assert_eq!(
        http.post(&url)
            .bearer_auth(&pending_bearer)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let (reconciled, restored) = bob
        .reconcile_gateway_key(&pending_input.request_id)
        .await
        .unwrap();
    assert_eq!(reconciled.state, GatewayKeyState::Active);
    assert_eq!(restored, pending_bearer);
    assert_eq!(
        prepared.secret(&bob_vault).await.unwrap().id(),
        original_secret.id()
    );
    assert_eq!(
        prepared.secret(&bob_vault).await.unwrap().expose_secret(),
        original_secret.expose_secret()
    );
    assert_ne!(
        http.post(&url)
            .bearer_auth(&restored)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    bob.revoke_gateway_key(&reconciled.id).await.unwrap();
    assert_eq!(
        http.post(&url)
            .bearer_auth(&restored)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let cancelled_id = hex::encode([0xed; 16]);
    let cancelled = {
        let local = bob.local().await.unwrap();
        let limited = restricted_session(&local, &[hellas_sdk::work::node::HOST_STATUS]).await;
        assert!(
            crate::gateway::issue_session(
                &local,
                GatewayKeyInput {
                    request_id: cancelled_id.clone(),
                    label: "cancel pending tool".into(),
                    period: crate::period::Period::Week
                },
                &limited
            )
            .await
            .is_err()
        );
        let prepared: crate::credentials::PreparedIssue = crate::vault::read(
            &b.path()
                .join("prepared-gateway-keys")
                .join(format!("{cancelled_id}.json")),
        )
        .unwrap()
        .unwrap();
        prepared
    };
    assert!(
        bob.gateway_keys()
            .await
            .unwrap()
            .iter()
            .any(|key| key.id == cancelled_id && key.state == GatewayKeyState::Pending)
    );
    bob.revoke_gateway_key(&cancelled_id).await.unwrap();
    assert!(
        bob.gateway_keys()
            .await
            .unwrap()
            .iter()
            .any(|key| key.id == cancelled_id && key.state == GatewayKeyState::Revoked)
    );
    assert!(
        bob_vault
            .get(&crate::credentials::PreparedIssue::key(
                cancelled.credential.id()
            ))
            .await
            .unwrap()
            .is_none()
    );
    assert!(bob.reconcile_gateway_key(&cancelled_id).await.is_err());
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
            .any(|key| key.id == first.id && key.state == GatewayKeyState::Revoked)
    );
    let listed = bob
        .gateway_keys()
        .await
        .unwrap()
        .into_iter()
        .find(|key| key.id == second_key.id)
        .unwrap();
    let (rotated, rotated_secret) = bob
        .rotate_gateway_key(
            &listed.id,
            GatewayKeyInput {
                request_id: hex::encode([0xcd; 16]).to_uppercase(),
                label: "rotated tool".into(),
                period: crate::period::Period::Week,
            },
        )
        .await
        .unwrap();
    assert_eq!(rotated.id, hex::encode([0xcd; 16]));
    assert_eq!(
        http.post(&url)
            .bearer_auth(&second)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_ne!(
        http.post(&url)
            .bearer_auth(&rotated_secret)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert!(
        b.path()
            .join("gateway-keys")
            .join(format!("{}.json", rotated.id))
            .exists()
    );
    assert!(
        !b.path()
            .join("gateway-keys")
            .join(format!("{}.json", rotated.id.to_uppercase()))
            .exists()
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
        service:"openai".into(),method:"responses".into(),presentation:FetchPresentation::Responses,policy_salt:hex::encode([9;32]),compute_credit_limit:40,delivery_credit_limit:40,fixed_price:10,dispatch_blocks:1,delivery_blocks:1,oracle_blocks:1,value:1000,reserve:200,fee_base:0,fee_slot:0,fee_proof:0,fee_lifetime:0,min_omit_response_blocks:hellas_sdk::kernel::MIN_OMIT_RESPONSE_BLOCKS,poll_ms:25,max_observation_age_ms:5000,
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
    let bytes = std::fs::read(crate::paid::pool_path(directory.path(), &pool.id).unwrap()).unwrap();
    let mut zero = pool.clone();
    zero.max_pending_requests = 0;
    let mut duplicate = pool.clone();
    let mut other = pool.providers[0].clone();
    other.bond = hex::encode([13; 32]);
    other.coins = vec![hex::encode([2; 32])];
    duplicate.providers.push(other.clone());
    let mut shared = pool.clone();
    other.provider = hellas_sdk::iroh::SecretKey::from_bytes(&[4; 32])
        .public()
        .to_string();
    other.coins = pool.providers[0].coins.clone();
    shared.providers.push(other);
    for invalid in [zero, duplicate, shared] {
        assert!(app.save_paid_pool(invalid).await.is_err());
        assert_eq!(
            std::fs::read(crate::paid::pool_path(directory.path(), &pool.id).unwrap()).unwrap(),
            bytes
        );
        let retained = local.clients.target(&local, &input).await.unwrap();
        assert!(Arc::ptr_eq(&original, &retained));
        assert_eq!(
            retained.backend.timeout(),
            std::time::Duration::from_secs(1)
        );
        drop(
            retained
                .backend
                .fetch(hellas_gateway::WorkFetchRequest {
                    provider: retained.provider,
                    service: retained.service.clone(),
                    method: retained.method.clone(),
                    body: br#"{"model":"fixture","input":"hello","max_output_tokens":8}"#.to_vec(),
                })
                .expect("a rejected pool edit must leave the original SDK backend usable"),
        );
        assert!(!directory.path().join("paid-channels/same-pool").exists());
    }
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

#[tokio::test(flavor = "multi_thread")]
async fn real_run_records_verified_success_cancellation_and_failed_incomplete_delivery() {
    let upstream = Upstream::start().await;
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let sa = tempfile::tempdir().unwrap();
    let sb = tempfile::tempdir().unwrap();
    let alice = AppState::with_vault(a.path(), Vault::fixture(sa.path()))
        .await
        .unwrap();
    let bob = Arc::new(
        AppState::with_vault(b.path(), Vault::fixture(sb.path()))
            .await
            .unwrap(),
    );
    alice.initialize_software("Alice".into()).await.unwrap();
    bob.initialize_software("Bob".into()).await.unwrap();
    let (request, _) = grant_https(&alice, &bob, &upstream, "alice", 121).await;
    let events = std::sync::Mutex::new(Vec::new());
    let completed = crate::run::execute(&bob, request.clone(), |event| {
        events.lock().unwrap().push(event)
    })
    .await
    .unwrap();
    let events = events.into_inner().unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ExecutionEvent::Verification { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event,ExecutionEvent::Finished {run_id} if run_id==&completed))
    );
    assert_eq!(
        bob.history
            .list(10)
            .unwrap()
            .iter()
            .find(|entry| entry.id == completed)
            .unwrap()
            .status,
        "complete"
    );
    upstream.started.notified().await;
    upstream.hold.store(true, Ordering::SeqCst);
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let active = bob.clone();
    let pending = request.clone();
    let cancelled = tokio::spawn(async move {
        crate::run::execute(&active, pending, |event| {
            let _ = sender.send(event);
        })
        .await
    });
    let ExecutionEvent::Started { run_id } = receiver.recv().await.unwrap() else {
        panic!("start event");
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        upstream.started.notified(),
    )
    .await
    .unwrap();
    bob.cancellations
        .lock()
        .await
        .get(&run_id)
        .unwrap()
        .send_replace(true);
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), cancelled)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        run_id
    );
    let remaining = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        remaining
            .iter()
            .any(|event| matches!(event, ExecutionEvent::Cancelled { .. }))
    );
    assert!(!remaining.iter().any(|event| matches!(
        event,
        ExecutionEvent::Verification { .. } | ExecutionEvent::Finished { .. }
    )));
    assert_eq!(
        bob.history
            .list(10)
            .unwrap()
            .iter()
            .find(|entry| entry.id == run_id)
            .unwrap()
            .status,
        "cancelled"
    );
    upstream.release.notify_one();
    bob.local().await.unwrap().clients.reset().await.unwrap();
    upstream.hold.store(false, Ordering::SeqCst);
    upstream.fail.store(true, Ordering::SeqCst);
    let events = std::sync::Mutex::new(Vec::new());
    let failed = crate::run::execute(&bob, request, |event| events.lock().unwrap().push(event))
        .await
        .unwrap();
    let events = events.into_inner().unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ExecutionEvent::Failed { .. }))
    );
    assert!(!events.iter().any(|event| matches!(
        event,
        ExecutionEvent::Verification { .. } | ExecutionEvent::Finished { .. }
    )));
    let history = bob.history.list(10).unwrap();
    let entry = history.iter().find(|entry| entry.id == failed).unwrap();
    assert_eq!(entry.status, "failed");
    assert!(!entry.error.is_empty());
    assert!(entry.verification.is_empty());
    bob.shutdown().await.unwrap();
    assert!(
        crate::run::execute(&bob, serde_json::from_str(&entry.request).unwrap(), |_| {})
            .await
            .is_err()
    );
    alice.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn dynamic_and_declarative_quota_edits_share_authority_revision_and_counters() {
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
    let (request, share) = grant_https(&alice, &bob, &upstream, "alice", 123).await;
    let quota = alice.quota(&share.allowances[0]).await.unwrap();
    assert_eq!(quota.requests_per_day, Some(3));
    let edit = QuotaInput {
        allowance: share.allowances[0].clone(),
        expected_revision: quota.revision,
        requests_per_day: 1,
        max_in_flight: 1,
    };
    alice.set_quota(edit.clone()).await.unwrap();
    let recipient = bob.local().await.unwrap();
    assert!(finished(
        &execute(&recipient.clients, &recipient, &request)
            .await
            .unwrap()
    ));
    assert!(
        execute(&recipient.clients, &recipient, &request)
            .await
            .is_err()
    );
    assert!(
        alice
            .set_quota(QuotaInput {
                requests_per_day: 2,
                ..edit
            })
            .await
            .is_err(),
        "stale dynamic revision is explicit"
    );
    let quota = alice.quota(&share.allowances[0]).await.unwrap();
    let view = alice.provider_view().await.unwrap();
    let desired = DesiredState {
        schema: 1,
        expected_provider_revision: view.revision,
        upstreams: vec![DesiredUpstream {
            service: "web".into(),
            method: "get".into(),
            hosts: vec!["127.0.0.1".into()],
            allow_private_addresses: true,
            keys: vec![],
        }],
        resources: view.resources,
        contacts: vec![],
        shares: vec![],
        gateway_keys: vec![],
        revoke_shares: vec![],
        revoke_gateway_keys: vec![],
        quotas: vec![QuotaInput {
            allowance: share.allowances[0].clone(),
            expected_revision: quota.revision,
            requests_per_day: 2,
            max_in_flight: 1,
        }],
        provider_enabled: None,
        gateway: None,
        gateway_enabled: None,
        paid_serve: None,
        paid_pools: vec![],
    };
    let preview = alice.preview_apply(desired.clone()).await.unwrap();
    assert!(!preview.requires_passkey);
    let report = alice.apply_document(&preview.id).await.unwrap();
    assert!(report.error.is_none());
    assert_eq!(report.applied.len(), 1);
    let quota = alice.quota(&share.allowances[0]).await.unwrap();
    assert_eq!(quota.requests_per_day, Some(2));
    assert_eq!(quota.revision, desired.quotas[0].expected_revision + 1);
    let preview = alice.preview_apply(desired).await.unwrap();
    let again = alice.apply_document(&preview.id).await.unwrap();
    assert!(again.error.is_none());
    assert_eq!(report.authority_revision, again.authority_revision);
    assert_eq!(
        quota.revision,
        alice.quota(&share.allowances[0]).await.unwrap().revision
    );
    assert!(finished(
        &execute(&recipient.clients, &recipient, &request)
            .await
            .unwrap()
    ));
    assert!(
        execute(&recipient.clients, &recipient, &request)
            .await
            .is_err()
    );
    assert_eq!(
        upstream.calls.load(Ordering::SeqCst),
        2,
        "editing the limit never resets consumed requests"
    );
    bob.shutdown().await.unwrap();
    alice.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn revocation_during_real_upstream_work_prevents_verified_delivery_and_future_admission() {
    let upstream = Upstream::start().await;
    upstream.hold.store(true, Ordering::SeqCst);
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let sa = tempfile::tempdir().unwrap();
    let sb = tempfile::tempdir().unwrap();
    let alice = AppState::with_vault(a.path(), Vault::fixture(sa.path()))
        .await
        .unwrap();
    let bob = Arc::new(
        AppState::with_vault(b.path(), Vault::fixture(sb.path()))
            .await
            .unwrap(),
    );
    alice.initialize_software("Alice".into()).await.unwrap();
    bob.initialize_software("Bob".into()).await.unwrap();
    let (request, share) = grant_https(&alice, &bob, &upstream, "alice", 125).await;
    let active = bob.clone();
    let pending = request.clone();
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = events.clone();
    let run = tokio::spawn(async move {
        crate::run::execute(&active, pending, |event| {
            captured.lock().unwrap().push(event)
        })
        .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        upstream.started.notified(),
    )
    .await
    .unwrap();
    alice.revoke_share(&share.id).await.unwrap();
    upstream.release.notify_one();
    let id = tokio::time::timeout(std::time::Duration::from_secs(20), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    {
        let events = events.lock().unwrap();
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ExecutionEvent::Failed { .. }))
        );
        assert!(!events.iter().any(|event| matches!(
            event,
            ExecutionEvent::Verification { .. } | ExecutionEvent::Finished { .. }
        )));
    }
    assert_eq!(
        bob.history
            .list(10)
            .unwrap()
            .iter()
            .find(|entry| entry.id == id)
            .unwrap()
            .status,
        "failed"
    );
    let local = bob.local().await.unwrap();
    assert!(execute(&local.clients, &local, &request).await.is_err());
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);
    bob.shutdown().await.unwrap();
    alice.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_closes_opening_for_an_admitted_run_before_lazy_paid_chain_start() {
    let directory = tempfile::tempdir().unwrap();
    let secrets = tempfile::tempdir().unwrap();
    let app = Arc::new(
        AppState::with_vault(directory.path(), Vault::fixture(secrets.path()))
            .await
            .unwrap(),
    );
    app.initialize_software("Client".into()).await.unwrap();
    let policy = paid_policy_fixture(directory.path()).await;
    trust::approve(
        directory.path(),
        TrustInput {
            id: "provider".into(),
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
    app.save_paid_pool(PaidPoolInput {
        id: "paid".into(),
        providers: vec![PaidProviderInput {
            provider: provider.clone(),
            addresses: vec![],
            trust_policy: "provider".into(),
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
    })
    .await
    .unwrap();
    let request = RunRequest {
        kind: RunKind::Fetch,
        input: br#"{"model":"fixture","input":"hello","max_output_tokens":8}"#
            .to_vec()
            .into_iter()
            .map(char::from)
            .collect(),
        client: WorkClientConfig {
            target: WorkTarget::Paid {
                pool_id: "paid".into(),
                provider,
                service: "openai".into(),
                method: "responses".into(),
            },
            trust_policy: "provider".into(),
        },
    };
    let local = app.local().await.unwrap();
    assert!(!local.clients.chain_initialized());
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let pause = barrier.clone();
    let (started, mut waiting) = tokio::sync::mpsc::unbounded_channel();
    let running = app.clone();
    let pending = request.clone();
    let run = tokio::spawn(async move {
        crate::run::execute(&running, pending, |event| {
            if let ExecutionEvent::Started { .. } = event {
                started.send(()).unwrap();
                pause.wait();
            }
        })
        .await
    });
    waiting.recv().await.unwrap();
    let quitting = app.clone();
    let shutdown = tokio::spawn(async move { quitting.shutdown().await });
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        local.clients.drained.notified(),
    )
    .await
    .unwrap();
    assert!(
        !shutdown.is_finished(),
        "Quit still owns the admitted run's permit"
    );
    assert!(!local.clients.chain_initialized());
    barrier.wait();
    let id = tokio::time::timeout(std::time::Duration::from_secs(5), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), shutdown)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
    );
    assert!(
        !local.clients.chain_initialized(),
        "no lazy chain may start after its only drain"
    );
    assert!(!local.socket.exists());
    assert!(!directory.path().join("chain").exists());
    assert!(!directory.path().join("paid-channels").exists());
    assert!(local.clients.target(&local, &request.client).await.is_err());
    let history = app.history.list(10).unwrap();
    assert_eq!(
        history.iter().find(|entry| entry.id == id).unwrap().status,
        "failed"
    );
}

fn desired_https(view: ProviderView) -> DesiredState {
    DesiredState {
        schema: 1,
        expected_provider_revision: view.revision,
        upstreams: vec![DesiredUpstream {
            service: "web".into(),
            method: "get".into(),
            hosts: vec!["127.0.0.1".into()],
            allow_private_addresses: true,
            keys: vec![],
        }],
        resources: view.resources,
        contacts: vec![],
        shares: vec![],
        gateway_keys: vec![],
        revoke_shares: vec![],
        revoke_gateway_keys: vec![],
        quotas: vec![],
        provider_enabled: None,
        gateway: None,
        gateway_enabled: None,
        paid_serve: None,
        paid_pools: vec![],
    }
}
#[tokio::test(flavor = "multi_thread")]
async fn stale_resource_and_retirement_previews_refuse_before_touching_live_work() {
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
    let (mut request, share) = grant_https(&alice, &bob, &upstream, "alice", 126).await;
    let desired = desired_https(alice.provider_view().await.unwrap());
    let mut update = desired.clone();
    if let ResourceKind::Https {
        max_response_bytes, ..
    } = &mut update.resources[0].resource
    {
        *max_response_bytes = 8192;
    }
    let stale_resource = alice.preview_apply(update).await.unwrap();
    let mut retire = desired.clone();
    retire.resources.clear();
    let stale_retire = alice.preview_apply(retire).await.unwrap();
    let mut newer = desired.resources[0].clone();
    if let ResourceKind::Https {
        max_response_bytes, ..
    } = &mut newer.resource
    {
        *max_response_bytes = 2048;
    }
    alice
        .configure_resource(desired.expected_provider_revision, newer)
        .await
        .unwrap();
    let mut body: hellas_rpc::http_fetch::HttpFetchRequest =
        serde_json::from_str(&request.input).unwrap();
    body.max_response_bytes = 2048;
    request.input = serde_json::to_string(&body).unwrap();
    // Refresh the allowance to the operator's newer resource before holding real work.
    alice
        .pause_allowance(&share.allowances[0], true)
        .await
        .unwrap();
    alice
        .pause_allowance(&share.allowances[0], false)
        .await
        .unwrap();
    bob.import_access(&alice.export_access(&share.id).await.unwrap())
        .await
        .unwrap();
    let local = alice.local().await.unwrap();
    let recipient = bob.local().await.unwrap();
    upstream.hold.store(true, Ordering::SeqCst);
    let active = recipient.clone();
    let pending = request.clone();
    let mut inflight =
        tokio::spawn(async move { execute(&active.clients, &active, &pending).await });
    tokio::select! {result=&mut inflight=>panic!("live Fetch failed before reaching the fixture: {result:?}"),result=tokio::time::timeout(std::time::Duration::from_secs(20),upstream.started.notified())=>{result.unwrap();}}

    let metadata = std::fs::read(a.path().join("provider-configuration.json")).unwrap();
    let revision = local
        .session(false)
        .await
        .unwrap()
        .status()
        .await
        .unwrap()
        .authority_revision;
    for preview in [stale_resource, stale_retire] {
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            alice.apply_document(&preview.id),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(error.to_string().contains("revision conflict"));
        assert_eq!(
            std::fs::read(a.path().join("provider-configuration.json")).unwrap(),
            metadata
        );
        assert_eq!(
            local
                .session(false)
                .await
                .unwrap()
                .status()
                .await
                .unwrap()
                .authority_revision,
            revision
        );
        assert_eq!(alice.status().await.unwrap().provider.state, "running");
        assert!(!inflight.is_finished());
    }
    upstream.release.notify_one();
    assert!(finished(&inflight.await.unwrap().unwrap()));
    bob.shutdown().await.unwrap();
    alice.shutdown().await.unwrap();
}
#[tokio::test(flavor = "multi_thread")]
async fn desired_key_resource_and_https_origin_switches_converge_and_reapply_idempotently() {
    let directory = tempfile::tempdir().unwrap();
    let secrets = tempfile::tempdir().unwrap();
    let vault = Vault::fixture(secrets.path());
    let app = AppState::with_vault(directory.path(), vault.clone())
        .await
        .unwrap();
    app.initialize_software("Owner".into()).await.unwrap();
    app.bind_provider(true).await.unwrap();
    app.configure_provider(
        0,
        ProviderChange::Endpoint {
            service: "web".into(),
            method: "get".into(),
            hosts: vec!["example.com".into()],
            allow_private_addresses: false,
        },
    )
    .await
    .unwrap();
    app.configure_provider(
        1,
        ProviderChange::AddKey {
            key: UpstreamKeyInput {
                service: "web".into(),
                method: "get".into(),
                key: "old".into(),
                origins: vec!["https://example.com".into()],
                paths: vec!["/".into()],
                methods: vec!["GET".into()],
                header: "authorization".into(),
                prefix: "Bearer ".into(),
                secret: "fixture-old-key".into(),
            },
        },
    )
    .await
    .unwrap();
    app.set_provider(true).await.unwrap();
    app.configure_resource(
        2,
        ResourceInput {
            name: "page".into(),
            service: "web".into(),
            method: "get".into(),
            resource: ResourceKind::Https {
                origin: "https://example.com".into(),
                paths: vec!["/".into()],
                methods: vec!["GET".into()],
                credential: Some("old".into()),
                max_response_bytes: 4096,
                tls_roots: vec![],
            },
        },
    )
    .await
    .unwrap();
    vault.set("new-source", b"fixture-new-key").await.unwrap();
    let view = app.provider_view().await.unwrap();
    let mut desired = desired_https(view);
    desired.upstreams[0].hosts = vec!["example.com".into()];
    desired.upstreams[0].allow_private_addresses = false;
    desired.upstreams[0].keys = vec![DesiredKey {
        name: "new".into(),
        origins: vec!["https://example.com".into()],
        paths: vec!["/".into()],
        methods: vec!["GET".into()],
        header: "authorization".into(),
        prefix: "Bearer ".into(),
        secret_reference: "new-source".into(),
    }];
    if let ResourceKind::Https { credential, .. } = &mut desired.resources[0].resource {
        *credential = Some("new".into());
    }
    let preview = app.preview_apply(desired.clone()).await.unwrap();
    let report = app.apply_document(&preview.id).await.unwrap();
    assert!(report.error.is_none(), "{:?}", report.error);
    assert_eq!(report.provider_revision, 6);
    assert_eq!(app.status().await.unwrap().provider.state, "running");
    let current = app.provider_view().await.unwrap();
    assert_eq!(current.upstreams[0].keys.len(), 1);
    assert_eq!(current.upstreams[0].keys[0].name, "new");
    let before = std::fs::read(directory.path().join("provider-configuration.json")).unwrap();
    let again = app.preview_apply(desired.clone()).await.unwrap();
    let reapplied = app.apply_document(&again.id).await.unwrap();
    assert!(reapplied.error.is_none());
    assert_eq!(report.authority_revision, reapplied.authority_revision);
    assert_eq!(
        before,
        std::fs::read(directory.path().join("provider-configuration.json")).unwrap()
    );
    desired.expected_provider_revision = report.provider_revision;
    desired.upstreams[0].hosts = vec!["other.example".into()];
    desired.upstreams[0].keys[0].origins = vec!["https://other.example".into()];
    if let ResourceKind::Https { origin, .. } = &mut desired.resources[0].resource {
        *origin = "https://other.example".into();
    }
    let preview = app.preview_apply(desired.clone()).await.unwrap();
    let report = app.apply_document(&preview.id).await.unwrap();
    assert!(report.error.is_none(), "{:?}", report.error);
    assert_eq!(
        app.provider_view().await.unwrap().upstreams[0].hosts,
        vec!["other.example"]
    );
    let preview = app.preview_apply(desired).await.unwrap();
    let again = app.apply_document(&preview.id).await.unwrap();
    assert!(again.error.is_none());
    assert_eq!(report.provider_revision, again.provider_revision);
    assert_eq!(report.authority_revision, again.authority_revision);
    app.shutdown().await.unwrap();
}

async fn restricted_session(
    local: &crate::control::LocalNode,
    operations: &[&str],
) -> crate::control::Session {
    use hellas_auth::authority::*;
    let mut scope = local.identity.public.owner_scope.clone();
    scope.issuance = None;
    scope.delegation_depth = 0;
    scope
        .permissions
        .retain(|permission| operations.contains(&permission.operation.as_str()));
    let (credential, secret) = Credential::prepare(
        scope,
        CredentialHolder::Node(
            NodeKey::from_public_key(local.identity.node.node_key().public_key()).unwrap(),
        ),
        local
            .session(false)
            .await
            .unwrap()
            .status()
            .await
            .unwrap()
            .generation,
    )
    .unwrap();
    local
        .session(true)
        .await
        .unwrap()
        .issue(&credential)
        .await
        .unwrap();
    let (tls, server) = hellas_sdk::node_control::profile::local_trust(
        rustls::pki_types::CertificateDer::from(local.identity.public.certificate.clone()),
        "localhost".into(),
    )
    .unwrap();
    let (client, _) = hellas_sdk::node_control::connect_local(
        &local.socket,
        tls,
        server,
        local.identity.public.authority,
        &secret,
        hellas_sdk::work::node::SessionKey::Passkey(local.identity.passkey.as_ref()),
    )
    .await
    .unwrap();
    crate::control::Session::new(client, local.authorization())
}
#[tokio::test(flavor = "multi_thread")]
async fn late_auth_refusal_reports_committed_share_budgets_and_allowance_exactly() {
    use hellas_sdk::work::model::grant::admin::{WorkControlCommand, WorkControlReply};
    let upstream = Upstream::start().await;
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let c = tempfile::tempdir().unwrap();
    let sa = tempfile::tempdir().unwrap();
    let sb = tempfile::tempdir().unwrap();
    let sc = tempfile::tempdir().unwrap();
    let alice = AppState::with_vault(a.path(), Vault::fixture(sa.path()))
        .await
        .unwrap();
    let bob = AppState::with_vault(b.path(), Vault::fixture(sb.path()))
        .await
        .unwrap();
    let carol = AppState::with_vault(c.path(), Vault::fixture(sc.path()))
        .await
        .unwrap();
    alice.initialize_software("Alice".into()).await.unwrap();
    bob.initialize_software("Bob".into()).await.unwrap();
    let carol_status = carol.initialize_software("Carol".into()).await.unwrap();
    grant_https(&alice, &bob, &upstream, "alice", 130).await;
    let contact = alice
        .add_contact(ContactInput {
            card: carol_status.identity.contact_card,
            holder: carol_status.identity.node_key.clone(),
            confirmed_holder: true,
            label: "Carol".into(),
        })
        .await
        .unwrap();
    let input = ShareInput {
        request_id: hex::encode([0xde; 16]),
        contact: contact.id,
        holder: carol_status.identity.node_key,
        allowances: vec![AllowanceInput {
            resource: "page".into(),
            requests_per_day: 7,
            max_in_flight: 1,
        }],
        period: crate::period::Period::Week,
        label: "partial share".into(),
    };
    let mut desired = desired_https(alice.provider_view().await.unwrap());
    desired.shares = vec![input.clone()];
    let local = alice.local().await.unwrap();
    let plan = crate::apply::plan(&local, desired).await.unwrap();
    let limited = restricted_session(
        &local,
        &[
            hellas_sdk::work::node::HOST_STATUS,
            hellas_sdk::work::work_store::grant::WORK_ALLOWANCE_READ,
            hellas_sdk::work::work_store::grant::WORK_BUDGET_CONFIGURE,
            hellas_sdk::work::work_store::grant::WORK_ALLOWANCE_CONFIGURE,
            hellas_auth::authority::GRANT_ADMINISTER,
        ],
    )
    .await;
    let report = crate::apply::execute_session(&local, plan, &limited)
        .await
        .unwrap();
    assert!(report.error.is_some());
    assert!(report.uncertain.is_empty());
    assert!(report.applied.iter().any(
        |operation| operation.contains("Work ConfigureBudget") && operation.contains("Subject")
    ));
    assert!(
        report
            .applied
            .iter()
            .any(|operation| operation.contains("Work Create"))
    );
    assert!(
        report
            .applied
            .iter()
            .any(|operation| operation.contains("Publish share intent"))
    );
    assert!(
        report
            .unapplied
            .iter()
            .any(|operation| operation.contains("Auth grant"))
    );
    assert!(
        report
            .unapplied
            .iter()
            .any(|operation| operation.starts_with("Finish "))
    );
    assert!(
        !report.unapplied.iter().any(|operation| operation
            .starts_with("Issue/reconcile exact share")
            && !operation.contains(": Auth grant")),
        "earlier Work commits cannot be reported wholly unapplied"
    );
    let saved: contacts::Shared = crate::vault::read(
        &a.path()
            .join("shares")
            .join(format!("{}.json", input.request_id)),
    )
    .unwrap()
    .unwrap();
    let owner = local.session(false).await.unwrap();
    let WorkControlReply::Status { allowance, .. } = owner
        .work(WorkControlCommand::Inspect {
            id: saved.allowances[0],
        })
        .await
        .unwrap()
    else {
        panic!("committed allowance");
    };
    let WorkControlReply::Budget { definition, .. } = owner
        .work(WorkControlCommand::InspectBudget {
            node: allowance.budget,
        })
        .await
        .unwrap()
    else {
        panic!("committed subject budget");
    };
    assert!(definition.limits.iter().any(|limit| limit.amount == 7));
    assert!(
        local
            .session(true)
            .await
            .unwrap()
            .inspect(hellas_auth::authority::InspectionQuery::Grant { id: saved.issue.id })
            .await
            .is_err()
    );
    bob.shutdown().await.unwrap();
    carol.shutdown().await.unwrap();
    alice.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn late_resource_refusal_reports_catalogue_and_restart_then_reapply_activates_authority() {
    use hellas_sdk::work::model::grant::admin::{WorkControlCommand, WorkControlReply};
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
    grant_https(&alice, &bob, &upstream, "alice", 131).await;
    let mut desired = desired_https(alice.provider_view().await.unwrap());
    if let ResourceKind::Https {
        max_response_bytes, ..
    } = &mut desired.resources[0].resource
    {
        *max_response_bytes = 8192;
    }
    let local = alice.local().await.unwrap();
    let original = local.composition.policy("page").unwrap();
    let plan = crate::apply::plan(&local, desired.clone()).await.unwrap();
    let limited = restricted_session(
        &local,
        &[
            hellas_sdk::work::node::HOST_STATUS,
            hellas_sdk::work::node::HOST_PROVIDER_CONFIGURE,
            hellas_sdk::work::work_store::grant::WORK_ALLOWANCE_READ,
        ],
    )
    .await;
    let report = crate::apply::execute_session(&local, plan, &limited)
        .await
        .unwrap();
    assert!(report.error.is_some());
    assert!(
        report
            .applied
            .iter()
            .any(|operation| operation.contains("Publish resource catalogue"))
    );
    assert!(
        report
            .applied
            .iter()
            .any(|operation| operation.contains("Set provider enabled=false"))
    );
    assert!(
        report
            .applied
            .iter()
            .any(|operation| operation.contains("Set provider enabled=true"))
    );
    assert!(
        report
            .unapplied
            .iter()
            .any(|operation| operation.contains("Work ConfigureResource"))
    );
    assert!(report.uncertain.is_empty());
    let WorkControlReply::Catalogue(active) = local
        .session(false)
        .await
        .unwrap()
        .work(WorkControlCommand::Catalogue)
        .await
        .unwrap()
    else {
        panic!("authority catalogue");
    };
    assert!(active.contains(&original));
    assert_ne!(local.composition.policy("page").unwrap(), original);
    desired.expected_provider_revision = alice.provider_view().await.unwrap().revision;
    if let ResourceKind::Https {
        max_response_bytes, ..
    } = &mut desired.resources[0].resource
    {
        *max_response_bytes = 16384;
    }
    let repeated = alice.preview_apply(desired.clone()).await.unwrap();
    let completed = alice.apply_document(&repeated.id).await.unwrap();
    assert!(completed.error.is_none(), "{:?}", completed.error);
    let WorkControlReply::Catalogue(active) = local
        .session(false)
        .await
        .unwrap()
        .work(WorkControlCommand::Catalogue)
        .await
        .unwrap()
    else {
        panic!("authority catalogue");
    };
    assert!(active.contains(&local.composition.policy("page").unwrap()));
    let policy = local.composition.policy("page").unwrap();
    assert_eq!(
        policy.revision.0, 2,
        "failed activation must not consume an authority revision"
    );
    assert_eq!(policy.https.as_ref().unwrap().max_response_bytes, 16384);
    let preview = alice.preview_apply(desired).await.unwrap();
    let again = alice.apply_document(&preview.id).await.unwrap();
    assert!(again.error.is_none());
    assert!(again.applied.is_empty());
    assert_eq!(again.authority_revision, completed.authority_revision);

    bob.shutdown().await.unwrap();
    alice.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn same_route_openai_resources_switch_credential_as_one_validated_group() {
    use hellas_sdk::work::model::grant::admin::{WorkControlCommand, WorkControlReply};
    let directory = tempfile::tempdir().unwrap();
    let secrets = tempfile::tempdir().unwrap();
    let vault = Vault::fixture(secrets.path());
    let app = AppState::with_vault(directory.path(), vault.clone())
        .await
        .unwrap();
    app.initialize_software("Owner".into()).await.unwrap();
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
    app.configure_provider(
        1,
        ProviderChange::AddKey {
            key: UpstreamKeyInput {
                service: "openai".into(),
                method: "responses".into(),
                key: "old".into(),
                origins: vec!["https://api.openai.com".into()],
                paths: vec!["/v1/responses".into()],
                methods: vec!["POST".into()],
                header: "authorization".into(),
                prefix: "Bearer ".into(),
                secret: "fixture-old-key".into(),
            },
        },
    )
    .await
    .unwrap();
    app.set_provider(true).await.unwrap();
    for (revision, name) in [(2, "small"), (3, "large")] {
        app.configure_resource(
            revision,
            ResourceInput {
                name: name.into(),
                service: "openai".into(),
                method: "responses".into(),
                resource: ResourceKind::OpenAi {
                    model: format!("fixture-{name}"),
                    max_output_tokens: 64,
                    credential: "old".into(),
                },
            },
        )
        .await
        .unwrap();
    }
    vault.set("new-source", b"fixture-new-key").await.unwrap();
    let view = app.provider_view().await.unwrap();
    let mut desired = desired_https(view);
    desired.upstreams = vec![DesiredUpstream {
        service: "openai".into(),
        method: "responses".into(),
        hosts: vec!["api.openai.com".into()],
        allow_private_addresses: false,
        keys: vec![DesiredKey {
            name: "new".into(),
            origins: vec!["https://api.openai.com".into()],
            paths: vec!["/v1/responses".into()],
            methods: vec!["POST".into()],
            header: "authorization".into(),
            prefix: "Bearer ".into(),
            secret_reference: "new-source".into(),
        }],
    }];
    for input in &mut desired.resources {
        if let ResourceKind::OpenAi { credential, .. } = &mut input.resource {
            *credential = "new".into();
        }
    }
    let preview = app.preview_apply(desired.clone()).await.unwrap();
    assert_eq!(
        preview
            .steps
            .iter()
            .filter(|step| step.contains("resource group"))
            .count(),
        1
    );
    let report = app.apply_document(&preview.id).await.unwrap();
    assert!(report.error.is_none(), "{:?}", report.error);
    assert_eq!(report.provider_revision, 7);
    assert_eq!(app.status().await.unwrap().provider.state, "running");
    let view = app.provider_view().await.unwrap();
    assert!(view.resources.iter().all(
        |input| matches!(&input.resource,ResourceKind::OpenAi {credential,..} if credential=="new")
    ));
    assert_eq!(view.upstreams[0].keys.len(), 1);
    assert_eq!(view.upstreams[0].keys[0].name, "new");
    let local = app.local().await.unwrap();
    let WorkControlReply::Catalogue(policies) = local
        .session(false)
        .await
        .unwrap()
        .work(WorkControlCommand::Catalogue)
        .await
        .unwrap()
    else {
        panic!("authority catalogue");
    };
    assert_eq!(policies.len(), 2);
    assert!(policies.iter().all(|policy| policy.revision.0 == 2));
    let bytes = std::fs::read(directory.path().join("provider-configuration.json")).unwrap();
    let preview = app.preview_apply(desired).await.unwrap();
    let again = app.apply_document(&preview.id).await.unwrap();
    assert!(again.error.is_none());
    assert!(again.applied.is_empty());
    assert_eq!(again.provider_revision, report.provider_revision);
    assert_eq!(again.authority_revision, report.authority_revision);
    assert_eq!(
        bytes,
        std::fs::read(directory.path().join("provider-configuration.json")).unwrap()
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn stopped_policy_two_start_and_unused_key_cleanup_converge_in_one_apply() {
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
    let (request, share) = grant_https(&alice, &bob, &upstream, "stopped", 231).await;
    let recipient = bob.local().await.unwrap();
    assert!(finished(
        &execute(&recipient.clients, &recipient, &request)
            .await
            .unwrap()
    ));
    let mut resource = alice.provider_view().await.unwrap().resources[0].clone();
    if let ResourceKind::Https {
        max_response_bytes, ..
    } = &mut resource.resource
    {
        *max_response_bytes = 8192;
    }
    alice.configure_resource(2, resource).await.unwrap();
    alice
        .configure_provider(
            3,
            ProviderChange::AddKey {
                key: UpstreamKeyInput {
                    service: "web".into(),
                    method: "get".into(),
                    key: "unused".into(),
                    origins: vec![upstream.url.clone()],
                    paths: vec!["/".into()],
                    methods: vec!["GET".into()],
                    header: "authorization".into(),
                    prefix: "Bearer ".into(),
                    secret: "unused-fixture-key".into(),
                },
            },
        )
        .await
        .unwrap();
    let local = alice.local().await.unwrap();
    let policy = local.composition.policy("page").unwrap();
    assert_eq!(policy.revision.0, 2);
    let quota = serde_json::to_value(alice.quota(&share.allowances[0]).await.unwrap()).unwrap();
    alice.set_provider(false).await.unwrap();
    let mut desired = desired_https(alice.provider_view().await.unwrap());
    desired.provider_enabled = Some(true);
    let preview = alice.preview_apply(desired.clone()).await.unwrap();
    let applied = alice.apply_document(&preview.id).await.unwrap();
    assert!(applied.error.is_none(), "{applied:?}");
    assert!(applied.unapplied.is_empty() && applied.uncertain.is_empty());
    assert_eq!(alice.provider_view().await.unwrap().revision, 5);
    assert!(
        alice.provider_view().await.unwrap().upstreams[0]
            .keys
            .is_empty()
    );
    assert_eq!(local.composition.policy("page").unwrap(), policy);
    let after = serde_json::to_value(alice.quota(&share.allowances[0]).await.unwrap()).unwrap();
    assert_eq!(after["revision"], quota["revision"]);
    assert_eq!(after, quota);
    assert_eq!(alice.status().await.unwrap().provider.state, "running");
    desired.expected_provider_revision = 5;
    let before = local
        .session(false)
        .await
        .unwrap()
        .status()
        .await
        .unwrap()
        .authority_revision;
    let preview = alice.preview_apply(desired).await.unwrap();
    let reapplied = alice.apply_document(&preview.id).await.unwrap();
    assert!(reapplied.error.is_none(), "{reapplied:?}");
    assert_eq!(alice.provider_view().await.unwrap().revision, 5);
    assert_eq!(
        local
            .session(false)
            .await
            .unwrap()
            .status()
            .await
            .unwrap()
            .authority_revision,
        before
    );
    bob.shutdown().await.unwrap();
    alice.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn transport_port_persists_on_restart_and_failed_bind_retries_without_owner_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let secrets = tempfile::tempdir().unwrap();
    let vault = Vault::fixture(secrets.path());
    let app = AppState::with_vault(directory.path(), vault.clone())
        .await
        .unwrap();
    let original = app.initialize_software("Owner".into()).await.unwrap();
    let probe = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let busy = probe.local_addr().unwrap().port();
    assert!(app.save_listen_port(Some(busy)).await.is_err());
    assert_eq!(app.status().await.unwrap().listen_port, None);
    assert!(
        app.local()
            .await
            .unwrap()
            .session(false)
            .await
            .unwrap()
            .status()
            .await
            .is_ok()
    );
    let available = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = available.local_addr().unwrap().port();
    drop(available);
    let saved = app.save_listen_port(Some(port)).await.unwrap();
    assert_eq!(saved.listen_port, Some(port));
    assert_eq!(saved.bound_addresses, original.bound_addresses);
    app.shutdown().await.unwrap();
    drop(app);
    // Simulate a race after saving. The failure remains correctable without
    // signing an offline generation recovery or replacing identity material.
    let held = std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).unwrap();
    let reopened = AppState::with_vault(directory.path(), vault.clone())
        .await
        .unwrap();
    assert!(!reopened.status().await.unwrap().identity.initialized);
    drop(held);
    let retried = reopened.retry_open().await.unwrap();
    assert!(retried.identity.initialized);
    assert_eq!(retried.identity.principal, original.identity.principal);
    assert_eq!(retried.identity.generation, original.identity.generation);
    assert_eq!(
        retried.identity.transport_id,
        original.identity.transport_id
    );
    assert!(retried.bound_addresses.iter().any(|a| {
        a.parse::<std::net::SocketAddr>()
            .is_ok_and(|a| a.is_ipv4() && a.port() == port)
    }));
    reopened.shutdown().await.unwrap();
    assert!(std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn openai_https_and_paid_share_one_host_and_invalid_paid_settings_preserve_work() {
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
    let (request, _) = grant_https(&alice, &bob, &upstream, "composition", 233).await;
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
    alice
        .configure_provider(
            3,
            ProviderChange::AddKey {
                key: UpstreamKeyInput {
                    service: "openai".into(),
                    method: "responses".into(),
                    key: "openai".into(),
                    origins: vec!["https://api.openai.com".into()],
                    paths: vec!["/v1/responses".into()],
                    methods: vec!["POST".into()],
                    header: "authorization".into(),
                    prefix: "Bearer ".into(),
                    secret: "fixture-no-external-requests".into(),
                },
            },
        )
        .await
        .unwrap();
    alice
        .configure_resource(
            4,
            ResourceInput {
                name: "model".into(),
                service: "openai".into(),
                method: "responses".into(),
                resource: ResourceKind::OpenAi {
                    model: "fixture".into(),
                    max_output_tokens: 64,
                    credential: "openai".into(),
                },
            },
        )
        .await
        .unwrap();
    let mut policy = paid_policy_fixture(a.path()).await;
    policy.service = "web".into();
    policy.method = "get".into();
    policy.presentation = FetchPresentation::Http;
    let paid = PaidServeInput {
        policy,
        routes: vec![],
    };
    let mut desired = desired_https(alice.provider_view().await.unwrap());
    let key = alice
        .provider_view()
        .await
        .unwrap()
        .upstreams
        .into_iter()
        .find(|u| u.service == "openai")
        .unwrap()
        .keys
        .remove(0);
    desired.upstreams.push(DesiredUpstream {
        service: "openai".into(),
        method: "responses".into(),
        hosts: vec!["api.openai.com".into()],
        allow_private_addresses: false,
        keys: vec![DesiredKey {
            name: "openai".into(),
            origins: vec!["https://api.openai.com".into()],
            paths: vec!["/v1/responses".into()],
            methods: vec!["POST".into()],
            header: "authorization".into(),
            prefix: "Bearer ".into(),
            secret_reference: key.secret_reference,
        }],
    });
    desired.paid_serve = Some(DesiredPaidServe {
        configuration: Some(paid.clone()),
    });
    let preview = alice.preview_apply(desired.clone()).await.unwrap();
    let applied = alice.apply_document(&preview.id).await.unwrap();
    assert!(applied.error.is_none(), "{applied:?}");
    desired.expected_provider_revision = 6;
    let bytes = std::fs::read(a.path().join("provider-configuration.json")).unwrap();
    let preview = alice.preview_apply(desired).await.unwrap();
    let applied = alice.apply_document(&preview.id).await.unwrap();
    assert!(applied.error.is_none(), "{applied:?}");
    assert!(applied.applied.is_empty());
    assert_eq!(
        std::fs::read(a.path().join("provider-configuration.json")).unwrap(),
        bytes
    );
    let local = alice.local().await.unwrap();
    assert!(local.chain.initialized());
    let peer = alice.status().await.unwrap().identity.transport_id;
    assert_eq!(alice.status().await.unwrap().provider.state, "running");
    assert_eq!(alice.provider_view().await.unwrap().resources.len(), 2);
    upstream.hold.store(true, Ordering::SeqCst);
    let recipient = bob.local().await.unwrap();
    let inflight =
        tokio::spawn(async move { execute(&recipient.clients, &recipient, &request).await });
    upstream.started.notified().await;
    let metadata = std::fs::read(a.path().join("provider-configuration.json")).unwrap();
    let mut invalid = paid.clone();
    invalid.policy.max_observation_age_ms = invalid.policy.poll_ms;
    let route = PaidRouteInput {
        peer: bob.status().await.unwrap().identity.transport_id,
        client: bob.status().await.unwrap().identity.node_key,
        bond: hex::encode([87; 32]),
    };
    let mut duplicate = paid.clone();
    duplicate.routes = vec![route.clone(), route.clone()];
    let mut missing = paid.clone();
    missing.routes = vec![route];
    let mut wrong = paid.clone();
    wrong.policy.presentation = FetchPresentation::Responses;
    for input in [invalid, duplicate, missing, wrong] {
        assert!(alice.configure_paid_serve(6, Some(input)).await.is_err());
        assert_eq!(
            std::fs::read(a.path().join("provider-configuration.json")).unwrap(),
            metadata
        );
        assert_eq!(alice.status().await.unwrap().provider.state, "running");
        assert_eq!(alice.status().await.unwrap().identity.transport_id, peer);
        assert!(!inflight.is_finished());
    }
    upstream.release.notify_one();
    assert!(finished(&inflight.await.unwrap().unwrap()));
    bob.shutdown().await.unwrap();
    alice.shutdown().await.unwrap();
    drop(local);
    drop(alice);
    let reopened = AppState::with_vault(a.path(), Vault::fixture(sa.path()))
        .await
        .unwrap();
    assert!(reopened.provider_view().await.unwrap().paid.is_some());
    assert!(!reopened.local().await.unwrap().chain.initialized());
    reopened.set_provider(true).await.unwrap();
    assert_eq!(reopened.status().await.unwrap().identity.transport_id, peer);
    assert_eq!(reopened.provider_view().await.unwrap().resources.len(), 2);
    reopened.shutdown().await.unwrap();
}

async fn grant_responses(
    provider: &AppState,
    recipient: &AppState,
    origin: &crate::responses_fixture::Origin,
    request_byte: u8,
) -> (RunRequest, ShareView) {
    provider.bind_provider(true).await.unwrap();
    let revision = provider.provider_view().await.unwrap().revision;
    provider
        .configure_provider(
            revision,
            ProviderChange::Endpoint {
                service: "openai".into(),
                method: "responses".into(),
                hosts: vec!["api.openai.com".into()],
                allow_private_addresses: false,
            },
        )
        .await
        .unwrap();
    provider
        .configure_provider(
            revision + 1,
            ProviderChange::AddKey {
                key: UpstreamKeyInput {
                    service: "openai".into(),
                    method: "responses".into(),
                    key: "fixture".into(),
                    origins: vec!["https://api.openai.com".into()],
                    paths: vec!["/v1/responses".into()],
                    methods: vec!["POST".into()],
                    header: "authorization".into(),
                    prefix: "Bearer ".into(),
                    secret: "fixture-only-secret".into(),
                },
            },
        )
        .await
        .unwrap();
    *provider
        .local()
        .await
        .unwrap()
        .composition
        .responses_origin
        .lock()
        .unwrap() = Some(origin.url.clone());
    provider.set_provider(true).await.unwrap();
    provider
        .configure_resource(
            revision + 2,
            ResourceInput {
                name: "model".into(),
                service: "openai".into(),
                method: "responses".into(),
                resource: ResourceKind::OpenAi {
                    model: "fixture".into(),
                    max_output_tokens: 64,
                    credential: "fixture".into(),
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
            label: "Responses recipient".into(),
        })
        .await
        .unwrap();
    let shared = provider
        .share(ShareInput {
            request_id: hex::encode([request_byte; 16]),
            contact: contact.id,
            holder: recipient_status.identity.node_key,
            allowances: vec![AllowanceInput {
                resource: "model".into(),
                requests_per_day: 20,
                max_in_flight: 2,
            }],
            period: crate::period::Period::Week,
            label: "Responses".into(),
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
            id: "responses".into(),
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
                hellas_reseller::responses_contract()
                    .unwrap()
                    .manifest_id()
                    .to_string(),
            ],
            app_id: String::new(),
            cd_hashes: vec![],
            reason: "Independent Responses fixture provider pin".into(),
        },
    )
    .unwrap();
    (
        RunRequest {
            kind: RunKind::Fetch,
            input: r#"{"model":"fixture","input":"hello","max_output_tokens":8,"store":false}"#
                .into(),
            client: WorkClientConfig {
                target: WorkTarget::Authorized {
                    access_id: access.id,
                    allowance: shared.allowances[0].clone(),
                    resource: "model".into(),
                },
                trust_policy: "responses".into(),
            },
        },
        shared,
    )
}
#[tokio::test(flavor = "multi_thread")]
async fn authorized_responses_gateway_enforces_keys_zdr_archive_revocation_and_shutdown_drain() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let sa = tempfile::tempdir().unwrap();
    let sb = tempfile::tempdir().unwrap();
    let alice = Arc::new(
        AppState::with_vault(a.path(), Vault::fixture(sa.path()))
            .await
            .unwrap(),
    );
    let bob = Arc::new(
        AppState::with_vault(b.path(), Vault::fixture(sb.path()))
            .await
            .unwrap(),
    );
    alice.initialize_software("Alice".into()).await.unwrap();
    bob.initialize_software("Bob".into()).await.unwrap();
    let origin = crate::responses_fixture::Origin::start().await;
    let (request, _) = grant_responses(&alice, &bob, &origin, 235).await;
    let events = std::sync::Mutex::new(vec![]);
    let run_id = crate::run::execute(&bob, request.clone(), |event| {
        events.lock().unwrap().push(event)
    })
    .await
    .unwrap();
    assert_eq!(bob.history.list(10).unwrap()[0].id, run_id);
    assert_eq!(
        bob.history.list(10).unwrap()[0].status,
        "complete",
        "{:?}",
        bob.history.list(10).unwrap()
    );
    assert!(
        events
            .into_inner()
            .unwrap()
            .iter()
            .any(|e| matches!(e, ExecutionEvent::Verification { .. }))
    );
    let (key, bearer) = bob
        .issue_gateway_key(GatewayKeyInput {
            request_id: hex::encode([236; 16]),
            label: "Responses HTTP".into(),
            period: crate::period::Period::Week,
        })
        .await
        .unwrap();
    let config = GatewayConfig {
        client: request.client,
        http_routes: vec![],
        port: Some(0),
        zdr: true,
    };
    bob.set_gateway(true, Some(config.clone())).await.unwrap();
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let address = bob.gateway_address().await.unwrap();
    let url = format!("{address}/v1/responses");
    let calls = origin.calls.load(Ordering::SeqCst);
    for token in [None, Some("invalid")] {
        let mut pending = http
            .post(&url)
            .header("content-type", "application/json")
            .body(request.input.clone());
        if let Some(token) = token {
            pending = pending.bearer_auth(token);
        }
        assert_eq!(pending.send().await.unwrap().status(), 401);
    }
    assert_eq!(origin.calls.load(Ordering::SeqCst), calls);
    let response = http
        .post(&url)
        .bearer_auth(&bearer)
        .header("content-type", "application/json")
        .body(request.input.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.text().await.unwrap().contains("hello"));
    assert!(!b.path().join("gateway-archive").exists());
    let mut archived = config;
    archived.zdr = false;
    bob.set_gateway(true, Some(archived)).await.unwrap();
    let url = format!("{}/v1/responses", bob.gateway_address().await.unwrap());
    let response = http
        .post(&url)
        .bearer_auth(&bearer)
        .header("content-type", "application/json")
        .body(request.input.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.text().await.unwrap().contains("hello"));
    assert!(
        std::fs::read_dir(b.path().join("gateway-archive"))
            .unwrap()
            .any(|e| e.unwrap().path().join("response.bin").exists())
    );
    bob.revoke_gateway_key(&key.id).await.unwrap();
    assert_eq!(
        http.post(&url)
            .bearer_auth(&bearer)
            .header("content-type", "application/json")
            .body(request.input.clone())
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let (_, next) = bob
        .issue_gateway_key(GatewayKeyInput {
            request_id: hex::encode([237; 16]),
            label: "drain".into(),
            period: crate::period::Period::Week,
        })
        .await
        .unwrap();
    origin.hold.store(true, Ordering::SeqCst);
    origin.started.notified().await;
    let pending = http
        .post(&url)
        .bearer_auth(&next)
        .header("content-type", "application/json")
        .body(request.input)
        .send();
    let response = tokio::spawn(pending);
    origin.started.notified().await;
    let closing = bob.clone();
    let quit = tokio::spawn(async move { closing.shutdown().await });
    tokio::task::yield_now().await;
    assert!(!quit.is_finished());
    origin.release.notify_one();
    assert_eq!(response.await.unwrap().unwrap().status(), 200);
    assert!(quit.await.unwrap().unwrap());
    assert!(http.get(&url).send().await.is_err());
    alice.shutdown().await.unwrap();
}

#[cfg(all(feature = "chain-tests", target_os = "linux"))]
#[path = "chain_acceptance.rs"]
mod chain_acceptance;
