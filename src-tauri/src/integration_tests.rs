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

fn paid_address(enrollment: u8, node: u8) -> String {
    hellas_sdk::client::ProviderAddress {
        enrollment: hellas_rpc::ContentId::from_bytes([enrollment; 32]),
        node: hellas_sdk::iroh::SecretKey::from_bytes(&[node; 32]).public(),
    }
    .to_string()
}

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
) -> anyhow::Result<Vec<hellas_gateway::WorkEvent<hellas_rpc::output::OutputEvent>>> {
    Ok(clients.fetch(local, request).await?.try_collect().await?)
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
    bob.initialize_software("Bob".into()).await.unwrap();
    let bs = bob.export_contact_card().await.unwrap();
    alice.bind_provider(true).await.unwrap();
    alice
        .configure_provider(
            alice.provider_view().await.unwrap().revision,
            ProviderChange::OpenAi {
                service: "openai".into(),
                method: "responses".into(),
                secret: Some("fixture-never-used-upstream-key".into()),
            },
        )
        .await
        .unwrap();
    alice.set_provider(true).await.unwrap();
    alice
        .configure_resource(
            alice.provider_view().await.unwrap().revision,
            ResourceInput {
                name: "responses".into(),
                service: "openai".into(),
                method: "responses".into(),
                resource: ResourceKind::OpenAi {
                    model: "fixture-model".into(),
                    max_output_tokens: 64,
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
        let limited = restricted_session(&local, &[hellas_authority::node::HOST_STATUS]).await;
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
        let limited = restricted_session(&local, &[hellas_authority::node::HOST_STATUS]).await;
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
    // Inspect stored names: APFS/NTFS may resolve paths with either casing.
    let names = std::fs::read_dir(b.path().join("gateway-keys"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(names.contains(&format!("{}.json", rotated.id)));
    assert!(!names.contains(&format!("{}.json", rotated.id.to_uppercase())));
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
            provider.provider_view().await.unwrap().revision,
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
            provider.provider_view().await.unwrap().revision,
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
    let recipient_status = recipient.export_contact_card().await.unwrap();
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
fn finished(events: &[hellas_gateway::WorkEvent<hellas_rpc::output::OutputEvent>]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            hellas_gateway::WorkEvent::Output(hellas_rpc::output::OutputEvent::Finished { .. })
        )
    })
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
    let (mut request, _shared) = grant_https(&alice, &bob, &upstream, "alice", 70).await;
    let recipient = bob.local().await.unwrap();
    let original = recipient
        .clients
        .target(&recipient, &request.client)
        .await
        .unwrap();
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 0);
    assert!(finished(
        &execute(&recipient.clients, &recipient, &request)
            .await
            .unwrap()
    ));
    let root = b
        .path()
        .join("grants")
        .join(format!(
            "p256-{}",
            hex::encode(recipient.identity.public.owner.as_bytes())
        ))
        .join("channels");
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
            .fetch(
                "run",
                Arc::new(hellas_gateway::jobs::Jobs::default())
                    .admit(&hellas_gateway::jobs::Request {
                        origin: hellas_gateway::jobs::Origin::Local,
                        path: "/run".into(),
                        method: "POST".into(),
                    })
                    .unwrap(),
                hellas_gateway::WorkFetchRequest {
                    service: original.service.clone(),
                    method: original.method.clone(),
                    body: request.input.as_bytes().to_vec(),
                }
            )
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

fn paid_limits() -> PaidFetchLimitsInput {
    PaidFetchLimitsInput {
        max_request_body_bytes: 64 << 10,
        max_output_events: 4096,
        max_output_bytes: 4 << 20,
        max_spool_bytes: 8 << 20,
        max_encoded_result_frame: 4 << 20,
        max_encoded_prepared_input: 128 << 10,
        execution_timeout_ms: 300_000,
    }
}
fn client_policy(policy: &PaidPolicyInput) -> PaidClientPolicyInput {
    PaidClientPolicyInput {
        chain: policy.chain.clone(),
        service: policy.service.clone(),
        method: policy.method.clone(),
        presentation: policy.presentation,
        fixed_price: policy.fixed_price,
    }
}
fn chain_input(profile: &hellas_genesis::ChainProfile) -> ChainInput {
    ChainInput::Custom(profile.to_file())
}
fn paid_policy_fixture() -> PaidPolicyInput {
    let profile = hellas_genesis::ChainProfile::known(hellas_genesis::Chain::Devnet)
        .with_validators(vec!["ws://127.0.0.1:9".into()])
        .unwrap();
    PaidPolicyInput {
        chain: chain_input(&profile),
        service: "openai".into(),
        method: "responses".into(),
        presentation: FetchPresentation::Responses,
        compute_credit_limit: 40,
        delivery_credit_limit: 40,
        fixed_price: 10,
        dispatch_blocks: 1,
        delivery_blocks: 1,
        oracle_blocks: 1,
        jobs_per_channel: 10,
        omission_bond: 101,
        max_stake_per_client: 10000,
        max_open_offers: 16,
        bond_lifetime_blocks: 2000,
        redelegate_payout: true,
        limits: paid_limits(),
    }
}

#[test]
fn standing_paid_policy_checks_capacity_limits_and_chain_bounds() {
    let input = paid_policy_fixture();
    let policy = crate::paid::standing_policy(&input).unwrap();
    assert_eq!(
        policy.expected_payment_values.value(),
        input.fixed_price * input.jobs_per_channel + input.omission_bond
    );
    assert_eq!(
        policy.expected_payment_values.close_fees(),
        hellas_chain::domain::KERNEL_FEES
    );
    assert_eq!(policy.expected_payment_values.reserve(), 0);
    let mut collateral = input.clone();
    collateral.omission_bond = input.jobs_per_channel * input.fixed_price;
    let mut offers = input.clone();
    offers.max_open_offers = 0;
    let mut limits = input.clone();
    limits.limits.max_output_events = 0;
    let mut lifetime = input.clone();
    lifetime.bond_lifetime_blocks = hellas_chain::domain::MAX_EDGE_LIFETIME_BLOCKS + 1;
    let mut overflow = input.clone();
    overflow.jobs_per_channel = u64::MAX;
    for invalid in [collateral, offers, limits, lifetime, overflow] {
        assert!(crate::paid::standing_policy(&invalid).is_err());
    }
}

#[tokio::test]
async fn paid_apple_approvals_and_journals_bind_the_full_provider_address() {
    let directory = tempfile::tempdir().unwrap();
    let secrets = tempfile::tempdir().unwrap();
    let app = AppState::with_vault(directory.path(), Vault::fixture(secrets.path()))
        .await
        .unwrap();
    app.initialize_software("Owner".into()).await.unwrap();
    let policy = paid_policy_fixture();
    let approve = |revision, enrollment| {
        trust::approve(
            directory.path(),
            TrustInput {
                id: "apple".into(),
                expected_revision: revision,
                genesis: hex::encode([enrollment; 32]),
                apple: true,
                app_id: "TEAM.test.gate".into(),
                cd_hashes: vec![hex::encode([7; 32])],
                reason: "Independent Apple enrollment and software pins".into(),
            },
        )
        .unwrap()
    };
    approve(0, 4);
    let mut pool = PaidPoolInput {
        id: "apple".into(),
        providers: vec![PaidProviderInput {
            provider: paid_address(4, 3),
            addresses: vec![],
            trust_policy: "apple".into(),
            acceptance_blocks: 16,
            terminal_blocks: 64,
            payment_blocks: 32,
            timeout_seconds: 1,
            policy: client_policy(&policy),
        }],
    };
    crate::paid::pool_snapshot(directory.path(), &pool).unwrap();
    let original = trust::resolve(directory.path(), "apple").unwrap();
    let anchor = &original;
    let config = crate::paid::config(&pool.providers[0].policy.chain, directory.path()).unwrap();
    let journal = config.chain.client_journals(&pool.providers[0].provider);
    assert_eq!(
        anchor.apple_app_attest.as_ref().unwrap().allowed_cd_hashes,
        vec![[7; 32]]
    );
    anchor
        .apple_app_attest
        .as_ref()
        .unwrap()
        .counter_store
        .advance(&[9; 33], 10)
        .unwrap();
    approve(1, 5);
    assert!(crate::paid::pool_snapshot(directory.path(), &pool).is_err());
    pool.providers[0].provider = paid_address(5, 3);
    crate::paid::pool_snapshot(directory.path(), &pool).unwrap();
    let replacement = trust::resolve(directory.path(), "apple").unwrap();
    assert_ne!(
        journal,
        config.chain.client_journals(&pool.providers[0].provider)
    );
    assert_ne!(original.expected_genesis, replacement.expected_genesis);
    assert_eq!(
        replacement
            .apple_app_attest
            .unwrap()
            .counter_store
            .advance(&[9; 33], 10),
        Err(hellas_attestation::AttestationError::Counter)
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn paid_pool_settings_refuse_duplicate_owners_and_preserve_retained_channels() {
    let directory = tempfile::tempdir().unwrap();
    let secrets = tempfile::tempdir().unwrap();
    let app = AppState::with_vault(directory.path(), Vault::fixture(secrets.path()))
        .await
        .unwrap();
    app.initialize_software("Paid client".into()).await.unwrap();
    fn chain_files(
        path: &std::path::Path,
    ) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
        let mut files = std::collections::BTreeMap::new();
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files.extend(chain_files(&path));
            } else {
                files.insert(path.clone(), std::fs::read(path).unwrap());
            }
        }
        files
    }
    let chain_home = directory.path().join("chains");
    let initial_chains = chain_files(&chain_home);
    let policy = paid_policy_fixture();
    trust::approve(
        directory.path(),
        TrustInput {
            id: "paid-provider".into(),
            expected_revision: 0,
            genesis: hex::encode([4; 32]),
            apple: false,
            app_id: String::new(),
            cd_hashes: vec![],
            reason: "Independent fixture pin".into(),
        },
    )
    .unwrap();
    let provider = paid_address(4, 3);
    let mut pool = PaidPoolInput {
        id: "same-pool".into(),
        providers: vec![PaidProviderInput {
            provider: provider.clone(),
            addresses: vec![],
            trust_policy: "paid-provider".into(),
            acceptance_blocks: 16,
            terminal_blocks: 64,
            payment_blocks: 32,
            timeout_seconds: 1,
            policy: client_policy(&policy),
        }],
    };
    let mut desired = desired_https(app.provider_view().await.unwrap());
    desired.upstreams.clear();
    desired.paid_pools = vec![pool.clone()];
    let preview = app.preview_apply(desired.clone()).await.unwrap();
    assert_eq!(preview.steps, vec!["Save typed paid pool same-pool"]);
    assert!(
        !crate::paid::pool_path(directory.path(), &pool.id)
            .unwrap()
            .exists()
    );
    let mut duplicate_node = pool.clone();
    duplicate_node.id = "other".into();
    duplicate_node.providers[0].provider = paid_address(5, 3);
    desired.paid_pools.push(duplicate_node);
    assert!(
        app.preview_apply(desired)
            .await
            .unwrap_err()
            .to_string()
            .contains("each paid provider must belong to one pool")
    );
    let applied = app.apply_document(&preview.id).await.unwrap();
    assert!(applied.error.is_none(), "{:?}", applied.error);
    let bytes = std::fs::read(crate::paid::pool_path(directory.path(), &pool.id).unwrap()).unwrap();
    assert_eq!(
        chain_files(&chain_home),
        initial_chains,
        "preview and save must not mutate chain state"
    );
    let mut zero = pool.clone();
    zero.providers[0].policy.fixed_price = 0;
    let mut duplicate = pool.clone();
    duplicate.providers.push(pool.providers[0].clone());
    let mut node_only = pool.clone();
    node_only.providers[0].provider = pool.providers[0]
        .provider
        .parse::<hellas_sdk::client::ProviderAddress>()
        .unwrap()
        .node
        .to_string();
    let mut wrong_enrollment = pool.clone();
    wrong_enrollment.providers[0].provider = paid_address(5, 3);
    assert!(
        crate::paid::pool_snapshot(directory.path(), &wrong_enrollment)
            .err()
            .unwrap()
            .to_string()
            .contains("different enrollment")
    );
    for invalid in [zero, duplicate, node_only, wrong_enrollment] {
        assert!(app.save_paid_pool(invalid).await.is_err());
        assert_eq!(
            std::fs::read(crate::paid::pool_path(directory.path(), &pool.id).unwrap()).unwrap(),
            bytes
        );
    }
    let mut changed_enrollment = pool.clone();
    changed_enrollment.id = "other".into();
    changed_enrollment.providers[0].provider = paid_address(5, 3);
    assert!(
        app.save_paid_pool(changed_enrollment)
            .await
            .unwrap_err()
            .to_string()
            .contains("already assigned")
    );
    let mut overlapping = pool.clone();
    overlapping.id = "other".into();
    assert!(
        app.save_paid_pool(overlapping.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("already assigned")
    );
    assert_eq!(
        chain_files(&chain_home),
        initial_chains,
        "refused pools must not mutate chain state"
    );
    let config = crate::paid::config(&pool.providers[0].policy.chain, directory.path()).unwrap();
    let retained = config.chain.client_journals(&pool.providers[0].provider);
    assert!(
        !retained.exists(),
        "validation and refused opening cannot create journals"
    );
    pool.providers[0].provider = paid_address(4, 4);
    app.save_paid_pool(pool).await.unwrap();
    // Journals are addressed by chain and provider, independently of the UI group.
    app.save_paid_pool(overlapping.clone()).await.unwrap();
    let reassigned = crate::paid::pool_snapshot(directory.path(), &overlapping).unwrap();
    let reopened_config =
        crate::paid::config(&overlapping.providers[0].policy.chain, directory.path()).unwrap();
    assert_eq!(
        reopened_config
            .chain
            .client_journals(&reassigned[0].input.provider),
        retained
    );
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
    let local = bob.local().await.unwrap();
    local
        .session(true)
        .await
        .unwrap()
        .edit(None, |document| {
            let ids: Vec<_> = document.backends.keys().cloned().collect();
            for id in ids {
                document
                    .remove_backend(&id)
                    .map_err(anyhow::Error::from_boxed)?;
            }
            Ok(())
        })
        .await
        .unwrap();
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
    let mut desired = DesiredState {
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
    desired.expected_provider_revision = alice.provider_view().await.unwrap().revision;
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
async fn shutdown_closes_opening_for_an_admitted_run_before_paid_target_start() {
    let directory = tempfile::tempdir().unwrap();
    let secrets = tempfile::tempdir().unwrap();
    let app = Arc::new(
        AppState::with_vault(directory.path(), Vault::fixture(secrets.path()))
            .await
            .unwrap(),
    );
    app.initialize_software("Client".into()).await.unwrap();
    let policy = paid_policy_fixture();
    trust::approve(
        directory.path(),
        TrustInput {
            id: "provider".into(),
            expected_revision: 0,
            genesis: hex::encode([4; 32]),
            apple: false,
            app_id: String::new(),
            cd_hashes: vec![],
            reason: "Independent fixture pin".into(),
        },
    )
    .unwrap();
    let provider = paid_address(4, 3);
    app.save_paid_pool(PaidPoolInput {
        id: "paid".into(),
        providers: vec![PaidProviderInput {
            provider: provider.clone(),
            addresses: vec![],
            trust_policy: "provider".into(),
            acceptance_blocks: 16,
            terminal_blocks: 64,
            payment_blocks: 32,
            timeout_seconds: 1,
            policy: client_policy(&policy),
        }],
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
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !local.core().jobs.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        !shutdown.is_finished(),
        "Quit still owns the admitted run's permit"
    );
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
    assert!(!local.socket.exists());
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

    let metadata = serde_json::to_value(local.core().watch().0.document).unwrap();
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
            serde_json::to_value(local.core().watch().0.document).unwrap(),
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
        hellas_authority::node::SessionKey::Passkey(local.identity.passkey.as_ref()),
    )
    .await
    .unwrap();
    crate::control::Session::new(client, local.authorization())
}
#[tokio::test(flavor = "multi_thread")]
async fn late_auth_refusal_reports_committed_share_budgets_and_allowance_exactly() {
    use hellas_authority::model::grant::admin::{WorkControlCommand, WorkControlReply};
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
    carol.initialize_software("Carol".into()).await.unwrap();
    let carol_status = carol.export_contact_card().await.unwrap();
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
            hellas_authority::node::HOST_STATUS,
            hellas_authority::work_store::grant::WORK_ALLOWANCE_READ,
            hellas_authority::work_store::grant::WORK_BUDGET_CONFIGURE,
            hellas_authority::work_store::grant::WORK_ALLOWANCE_CONFIGURE,
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

async fn resource_policy(
    local: &crate::control::LocalNode,
    name: &str,
) -> anyhow::Result<hellas_authority::model::grant::records::GrantPolicy> {
    use hellas_authority::model::grant::admin::{WorkControlCommand, WorkControlReply};
    let WorkControlReply::Catalogue(policies) = local
        .session(false)
        .await?
        .work(WorkControlCommand::Catalogue)
        .await?
    else {
        anyhow::bail!("expected resource catalogue")
    };
    policies
        .into_iter()
        .find(|policy| policy.name == name)
        .ok_or_else(|| anyhow::anyhow!("resource is not configured"))
}

#[tokio::test(flavor = "multi_thread")]
async fn provider_restart_publishes_configured_resource_without_runtime_catalogue_mutation() {
    use hellas_authority::model::grant::admin::{WorkControlCommand, WorkControlReply};
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
    let original = resource_policy(&local, "page").await.unwrap();
    let plan = crate::apply::plan(&local, desired.clone()).await.unwrap();
    let limited = restricted_session(
        &local,
        &[
            hellas_authority::node::HOST_STATUS,
            hellas_authority::node::HOST_CONFIGURE,
            hellas_authority::work_store::grant::WORK_ALLOWANCE_READ,
        ],
    )
    .await;
    let report = crate::apply::execute_session(&local, plan, &limited)
        .await
        .unwrap();
    assert!(report.error.is_none(), "{:?}", report.error);
    assert!(report.uncertain.is_empty());
    let WorkControlReply::Catalogue(active) = local
        .session(false)
        .await
        .unwrap()
        .work(WorkControlCommand::Catalogue)
        .await
        .unwrap()
    else {
        panic!("authority catalogue")
    };
    assert!(!active.contains(&original));
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
    let policy = active.iter().find(|p| p.name == "page").unwrap();
    assert_eq!(
        policy.revision.0, 3,
        "both successfully published configurations advance the revision"
    );
    assert_eq!(policy.https.as_ref().unwrap().max_response_bytes, 16384);
    desired.expected_provider_revision = alice.provider_view().await.unwrap().revision;
    let preview = alice.preview_apply(desired).await.unwrap();
    let again = alice.apply_document(&preview.id).await.unwrap();
    assert!(again.error.is_none());
    assert!(again.applied.is_empty());
    assert_eq!(again.authority_revision, completed.authority_revision);

    bob.shutdown().await.unwrap();
    alice.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn transport_port_persists_and_failed_bind_is_repaired_through_control() {
    let directory = tempfile::tempdir().unwrap();
    let secrets = tempfile::tempdir().unwrap();
    let vault = Vault::fixture(secrets.path());
    let app = AppState::with_vault(directory.path(), vault.clone())
        .await
        .unwrap();
    let original = app.initialize_software("Owner".into()).await.unwrap();
    app.bind_provider(true).await.unwrap();
    app.set_provider(true).await.unwrap();
    let bound = app.status().await.unwrap().bound_addresses;
    let probe = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let busy = probe.local_addr().unwrap().port();
    assert!(app.save_listen_port(Some(busy)).await.is_err());
    assert_eq!(app.status().await.unwrap().listen_port, Some(0));
    assert_eq!(app.status().await.unwrap().bound_addresses, bound);
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
    assert!(saved.bound_addresses.iter().any(|a| {
        a.parse::<std::net::SocketAddr>()
            .is_ok_and(|a| a.port() == port)
    }));
    app.shutdown().await.unwrap();
    drop(app);
    // Simulate a race after saving. The failure remains correctable without
    // signing an offline generation recovery or replacing identity material.
    let held = std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).unwrap();
    let reopened = AppState::with_vault(directory.path(), vault.clone())
        .await
        .unwrap();
    let failed = reopened.status().await.unwrap();
    assert!(failed.identity.initialized);
    assert_eq!(failed.provider.state, "error");
    assert!(!failed.provider.detail.is_empty());
    assert!(failed.bound_addresses.is_empty());
    assert!(
        reopened
            .local()
            .await
            .unwrap()
            .session(false)
            .await
            .unwrap()
            .status()
            .await
            .is_ok()
    );
    drop(held);
    let repaired = reopened.save_listen_port(Some(port)).await.unwrap();
    assert_eq!(repaired.provider.state, "running");
    assert!(repaired.provider.detail.is_empty());
    assert!(repaired.identity.initialized);
    assert_eq!(repaired.identity.principal, original.identity.principal);
    assert_eq!(repaired.identity.generation, original.identity.generation);
    assert_eq!(
        repaired.identity.transport_id,
        original.identity.transport_id
    );
    assert!(repaired.bound_addresses.iter().any(|a| {
        a.parse::<std::net::SocketAddr>()
            .is_ok_and(|a| a.is_ipv4() && a.port() == port)
    }));
    reopened.shutdown().await.unwrap();
    assert!(std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).is_ok());
}

#[cfg(all(feature = "chain-tests", target_os = "linux"))]
#[path = "chain_acceptance.rs"]
mod chain_acceptance;

#[tokio::test(flavor = "multi_thread")]
async fn provider_editor_commits_one_document_and_never_echoes_keys() {
    use hellas_sdk::daemon_config::{BackendKind, Upstream};
    let directory = tempfile::tempdir().unwrap();
    let secrets = tempfile::tempdir().unwrap();
    let vault = Vault::fixture(secrets.path());
    let app = AppState::with_vault(directory.path(), vault.clone())
        .await
        .unwrap();
    app.initialize_software("Owner".into()).await.unwrap();
    let mut desired = desired_https(app.provider_view().await.unwrap());
    desired.upstreams[0].hosts = vec!["example.com".into()];
    desired.upstreams[0].allow_private_addresses = false;
    desired.upstreams[0].keys.push(DesiredKey {
        name: "api".into(),
        origins: vec!["https://example.com".into()],
        paths: vec!["/".into()],
        methods: vec!["GET".into()],
        header: "authorization".into(),
        prefix: "Bearer ".into(),
        secret: Some("private-backend-key".into()),
    });
    desired.resources.push(ResourceInput {
        name: "page".into(),
        service: "web".into(),
        method: "get".into(),
        resource: ResourceKind::Https {
            origin: "https://example.com".into(),
            paths: vec!["/".into()],
            methods: vec!["GET".into()],
            credential: Some("api".into()),
            max_response_bytes: 4096,
            tls_roots: vec![],
        },
    });
    let preview = app.preview_apply(desired.clone()).await.unwrap();
    assert_eq!(preview.steps.len(), 1);
    assert!(
        !serde_json::to_string(&preview)
            .unwrap()
            .contains("private-backend-key")
    );
    let report = app.apply_document(&preview.id).await.unwrap();
    assert!(report.error.is_none(), "{:?}", report.error);
    assert_eq!(report.provider_revision, 1);
    let local = app.local().await.unwrap();
    let session = local.session(true).await.unwrap();
    let snapshot = session.view().await.unwrap();
    let public = serde_json::to_value(&snapshot.document).unwrap();
    assert!(!public.to_string().contains("private-backend-key"));
    assert_eq!(snapshot.document.resources.len(), 1);
    assert_eq!(snapshot.document.backends.len(), 1);
    let stored = local.coordinator.configuration().await.unwrap().unwrap();
    assert_eq!(
        stored.secrets.0["web/get"]["api"],
        "Bearer private-backend-key"
    );
    let mut invalid = snapshot.document;
    let BackendKind::Fetch {
        upstream:
            Upstream::Http {
                allowed_hosts,
                credentials,
                ..
            },
        ..
    } = &mut invalid.backends.get_mut("web/get").unwrap().kind
    else {
        panic!("HTTP backend")
    };
    *allowed_hosts = vec!["invalid host".into()];
    credentials.get_mut("api").unwrap().secret = Some("rejected-rotation".into());
    assert!(
        session
            .configure(snapshot.revision.0, invalid)
            .await
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(session.view().await.unwrap().document).unwrap(),
        public
    );
    assert_eq!(
        local.coordinator.configuration().await.unwrap().unwrap(),
        stored
    );
    desired.expected_provider_revision = 1;
    desired.upstreams[0].keys[0].secret = None;
    let unchanged = app.preview_apply(desired).await.unwrap();
    assert!(unchanged.steps.is_empty());
    drop(session);
    app.shutdown().await.unwrap();
    drop(local);
    drop(app);
    let reopened = AppState::with_vault(directory.path(), vault).await.unwrap();
    assert_eq!(reopened.provider_view().await.unwrap().revision, 1);
    assert_eq!(reopened.provider_view().await.unwrap().resources.len(), 1);
    reopened.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn native_run_and_http_share_the_configured_grant_channel_and_quota() {
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
    let (request, _) = grant_https(&alice, &bob, &upstream, "alice", 231).await;
    let local = bob.local().await.unwrap();
    assert!(finished(
        &execute(&local.clients, &local, &request).await.unwrap()
    ));
    let config = GatewayConfig {
        client: request.client.clone(),
        http_routes: vec![HttpRouteInput {
            path: "/proxy".into(),
            method: "GET".into(),
            url: format!("{}/", upstream.url),
            credential: None,
            max_response_bytes: 4096,
            tls_roots: vec![upstream.root.clone()],
        }],
        port: Some(0),
        zdr: true,
    };
    bob.set_gateway(true, Some(config.clone())).await.unwrap();
    let native = local.clients.target(&local, &request.client).await.unwrap();
    let watched = local.session(false).await.unwrap().view().await.unwrap();
    assert_eq!(watched.document.backends.len(), 1);
    assert!(
        watched
            .document
            .routes
            .iter()
            .all(|route| route.backends == [native.id.clone()])
    );
    let (key, bearer) = bob
        .issue_gateway_key(GatewayKeyInput {
            request_id: hex::encode([232; 16]),
            label: "HTTP".into(),
            period: crate::period::Period::Week,
        })
        .await
        .unwrap();
    let url = format!("{}/proxy", bob.gateway_address().await.unwrap());
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    assert_eq!(client.get(&url).send().await.unwrap().status(), 401);
    let response = client.get(&url).bearer_auth(&bearer).send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "hello");
    assert!(finished(
        &execute(&local.clients, &local, &request).await.unwrap()
    ));
    assert!(execute(&local.clients, &local, &request).await.is_err());
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 3);
    assert!(!b.path().join("gateway-archive").exists());
    let before = serde_json::to_value(local.core().watch().0.document).unwrap();
    let mut invalid = config;
    invalid.http_routes[0].path = "/bad?path".into();
    assert!(bob.set_gateway(true, Some(invalid)).await.is_err());
    assert_eq!(
        serde_json::to_value(local.core().watch().0.document).unwrap(),
        before
    );
    bob.revoke_gateway_key(&key.id).await.unwrap();
    assert_eq!(
        client
            .get(&url)
            .bearer_auth(&bearer)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    bob.shutdown().await.unwrap();
    alice.shutdown().await.unwrap();
}
