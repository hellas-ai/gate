//! Funded Gate acceptance using the SDK's six independently executing validators.
//! The SDK fixture orders blocks; kernel execution, signatures and transports are real.
use super::*;
use hellas_chain::{
    LightClient as _,
    test_support::devnet::{Devnet, DevnetConfig, GenesisOwner, OwnerKind, run},
};
use hellas_sdk::kernel::{CoinId, Key};
use std::time::Duration;

fn policy(devnet: &Devnet) -> PaidPolicyInput {
    let genesis = devnet.genesis();
    PaidPolicyInput {
        chain: ChainInput {
            network: genesis.network_id.clone(),
            genesis_digest: hellas_auth::Digest::from(devnet.genesis_payload().0).to_string(),
            threshold_identity: hex::encode(&devnet.consensus_info().threshold_identity),
            validators: devnet
                .light_client_urls()
                .into_iter()
                .map(|url| format!(" \t{}\n", url.replacen("ws://", "WS://", 1)))
                .collect(),
            committee: genesis
                .validators
                .iter()
                .map(|v| GenesisValidatorInput {
                    public_key: v.public_key.clone(),
                    label: v.label.clone(),
                })
                .collect(),
            allocations: genesis
                .allocations
                .iter()
                .map(|a| GenesisAllocationInput {
                    address: a.address.clone(),
                    balance: a.balance,
                })
                .collect(),
        },
        service: "web".into(),
        method: "get".into(),
        presentation: FetchPresentation::Http,
        policy_salt: hex::encode([9; 32]),
        compute_credit_limit: 40,
        delivery_credit_limit: 40,
        fixed_price: 10,
        dispatch_blocks: 16,
        delivery_blocks: 16,
        oracle_blocks: 16,
        value: 10000,
        reserve: 0,
        fee_base: 0,
        fee_slot: 0,
        fee_proof: 0,
        fee_lifetime: 0,
        min_omit_response_blocks: hellas_sdk::kernel::MIN_OMIT_RESPONSE_BLOCKS,
        poll_ms: 50,
        max_observation_age_ms: 60000,
    }
}
async fn sealing_while<T>(devnet: &mut Devnet, work: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(120), async {
        tokio::pin!(work);
        loop {
            tokio::select! {
                result = &mut work => return result,
                _ = tokio::time::sleep(Duration::from_millis(100)) => {},
            }
            devnet.seal().await;
        }
    })
    .await
    .expect("one SDK setup or owner operation completes between fully applied blocks")
}

async fn delegate(
    local: &crate::control::LocalNode,
    policy: &PaidPolicyInput,
    native: Option<&hellas_sdk::kernel::Secp256k1Signer>,
) -> CoinId {
    use hellas_sdk::coin_owner::OwnerReshape;
    use hellas_sdk::work::authenticator::kernel_auth;
    let config = crate::paid::config(policy, local.directory.join("unused-plan")).unwrap();
    let node = local.chain.get(&config).await.unwrap();
    let view = tokio::time::timeout(Duration::from_secs(90), node.wait_ready())
        .await
        .expect("six local validators must finalize")
        .unwrap();
    let client = view.client();
    let owner = native.map_or_else(
        || Key::from_bytes(*local.identity.public.owner.as_bytes()),
        |key| key.party_key(),
    );
    let listing = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match client.get_coins_by_owner(owner.into()).await {
                Ok(Some(listing)) => break listing,
                Err(hellas_chain::QueryError::StateUnavailable(_)) | Ok(None) => {
                    tokio::time::sleep(Duration::from_millis(50)).await
                }
                other => panic!("coin snapshot: {other:?}"),
            }
        }
    })
    .await
    .unwrap();
    let coin = CoinId::from_bytes(listing.coins[0].0.0);
    let plan = OwnerReshape::prepare(
        &client,
        config.chain.network,
        owner,
        &[coin],
        Some(Key::from_bytes(
            local
                .identity
                .node
                .node_key()
                .public_key()
                .bytes()
                .try_into()
                .unwrap(),
        )),
    )
    .await
    .unwrap();
    let output = plan.output_ids().iter().next().copied().unwrap();
    let auth = if let Some(native) = native {
        hellas_sdk::kernel::Auth::native(native.sign(plan.payload_hash()))
    } else {
        let assertion = local
            .identity
            .passkey
            .assert(hellas_auth::Digest::from(plan.payload_hash().to_bytes()))
            .await
            .unwrap();
        kernel_auth(&local.identity.passkey.public_key().unwrap(), &assertion).unwrap()
    };
    client
        .submit_tx(hellas_chain::domain::Transaction::Kernel(
            plan.authorize(auth).unwrap(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match client.get_coins_by_owner(owner.into()).await {
                Ok(Some(listing))
                    if listing
                        .coins
                        .iter()
                        .any(|(id, _)| id.0 == output.to_bytes()) =>
                {
                    break;
                }
                Ok(_) | Err(hellas_chain::QueryError::StateUnavailable(_)) => {
                    tokio::time::sleep(Duration::from_millis(100)).await
                }
                Err(error) => panic!("delegation confirmation: {error:?}"),
            }
        }
    })
    .await
    .unwrap();
    output
}

static CHAIN_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn funded_gateway(runtime: commonware_runtime::tokio::Context, responses: bool) {
    let _network_slot = CHAIN_TEST.lock().await;
    let _ = tracing_subscriber::fmt()
        .with_env_filter("hellas_sdk=info,hellas_gateway=debug,hellas_work=info,hellas_chain=warn")
        .with_ansi(false)
        .try_init();
    const ALICE: GenesisOwner = GenesisOwner {
        kind: OwnerKind::Passkey,
        seed: 11,
        balance: 10000,
    };
    const BOB: GenesisOwner = GenesisOwner {
        kind: OwnerKind::Passkey,
        seed: 13,
        balance: 10000,
    };
    const NATIVE: GenesisOwner = GenesisOwner {
        kind: OwnerKind::Native,
        seed: 83,
        balance: 10000,
    };
    let mut devnet = Devnet::start(
        runtime,
        DevnetConfig {
            name: "gate_paid",
            owners: vec![ALICE, BOB, NATIVE],
            full_nodes: 2,
        },
    )
    .await;
    devnet.seal().await;
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let sa = tempfile::tempdir().unwrap();
    let sb = tempfile::tempdir().unwrap();
    let av = Vault::fixture(sa.path());
    let bv = Vault::fixture(sb.path());
    crate::identity::Identity::fixture(a.path(), &av, "Alice".into(), ALICE.scalar())
        .await
        .unwrap();
    crate::identity::Identity::fixture(b.path(), &bv, "Bob".into(), BOB.scalar())
        .await
        .unwrap();
    let alice = AppState::with_vault(a.path(), av).await.unwrap();
    let bob = AppState::with_vault(b.path(), bv).await.unwrap();
    let ast = alice.status().await.unwrap();
    let bst = bob.status().await.unwrap();
    let al = alice.local().await.unwrap();
    let bl = bob.local().await.unwrap();
    al.chain.fixture(devnet.full_node(0).clone()).unwrap();
    bl.chain.fixture(devnet.full_node(1).clone()).unwrap();
    assert_eq!(ast.identity.principal, hex::encode(ALICE.key().to_bytes()));
    assert_eq!(bst.identity.principal, hex::encode(BOB.key().to_bytes()));
    let native = hellas_sdk::kernel::Secp256k1Signer::from_secret_scalar(NATIVE.scalar()).unwrap();
    let mut policy = policy(&devnet);
    let upstream = Upstream::start().await;
    let origin = if responses {
        Some(crate::responses_fixture::Origin::start().await)
    } else {
        None
    };
    let (mut request, _) = if let Some(origin) = &origin {
        policy.service = "openai".into();
        policy.method = "responses".into();
        policy.presentation = FetchPresentation::Responses;
        grant_responses(&alice, &bob, origin, 232).await
    } else {
        grant_https(&alice, &bob, &upstream, "paid", 232).await
    };
    let revision = alice.provider_view().await.unwrap().revision;
    let stake = sealing_while(&mut devnet, Box::pin(delegate(&al, &policy, None))).await;
    let payment = sealing_while(&mut devnet, Box::pin(delegate(&bl, &policy, None))).await;
    let native_coin =
        sealing_while(&mut devnet, Box::pin(delegate(&bl, &policy, Some(&native)))).await;
    let client_node = devnet.full_node(1).clone();
    eprintln!(
        "real-chain fixture: native and passkey owner delegations finalized through the same SDK"
    );
    alice
        .configure_paid_serve(
            revision,
            Some(PaidServeInput {
                policy: policy.clone(),
                routes: vec![],
            }),
        )
        .await
        .unwrap();
    let height = devnet.height();
    let input = ProvisionInput {
        expected_provider_revision: revision + 1,
        contact: bst.identity.principal,
        client_node: bst.identity.node_key.clone(),
        client_peer: bob.paid_client_peer("real").await.unwrap(),
        addresses: ast
            .bound_addresses
            .iter()
            .filter_map(|s| s.parse::<std::net::SocketAddr>().ok())
            .filter(|a| a.is_ipv4())
            .map(|a| format!("127.0.0.1:{}", a.port()))
            .collect(),
        redelegate_payout: true,
        stake_coins: vec![hex::encode(stake.to_bytes())],
        bond_timeout: height + 2000,
        timeout_payout: 10000,
        max_job_price: 10,
    };
    let preview = alice.provision_paid(input.clone(), false).await.unwrap();
    assert!(preview.signed_offer.is_none());
    assert!(!a.path().join("paid-provider").exists());
    assert_eq!(preview.owner, ast.identity.principal);
    assert_eq!(preview.payout_delegate, Some(ast.identity.node_key));
    assert!(
        alice
            .provision_paid(input.clone(), true)
            .await
            .unwrap_err()
            .to_string()
            .contains("stop and drain")
    );
    alice.set_provider(false).await.unwrap();
    let mut wrong = input.clone();
    wrong.stake_coins = vec![hex::encode(native_coin.to_bytes())];
    let refused = alice.provision_paid(wrong, true).await.unwrap_err();
    assert!(
        matches!(refused.downcast_ref::<hellas_sdk::coin_owner::CoinOwnerError>(),Some(hellas_sdk::coin_owner::CoinOwnerError::Missing(id)) if *id==native_coin),
        "{refused:#}"
    );
    assert!(!a.path().join("paid-provider").exists());
    let offered = alice.provision_paid(input.clone(), true).await.unwrap();
    eprintln!("real-chain fixture: offer durably provisioned");
    assert_eq!(offered.bond, preview.bond);
    assert!(offered.signed_offer.is_some());
    let signed = hellas_sdk::work::model::offer::SignedPaidOffer::decode(
        &STANDARD
            .decode(offered.signed_offer.as_ref().unwrap())
            .unwrap(),
    )
    .unwrap();
    let terms = signed.offer().proposal.bond_terms();
    let maker = hellas_sdk::kernel::Secp256k1Signer::from_secret_scalar(
        al.identity.node.node_secret_bytes(),
    )
    .unwrap()
    .party_key();
    let owner = Key::from_bytes(*al.coordinator.owner().await.unwrap().as_bytes());
    assert_eq!(terms.parties.maker().owner(), owner);
    assert_eq!(terms.parties.maker().signer(), maker);
    assert_eq!(terms.parties.maker().payout_delegate(), Some(maker));
    assert_eq!(
        terms.parties.taker().owner(),
        Key::from_bytes(*bl.coordinator.owner().await.unwrap().as_bytes())
    );
    let payout = terms.timeout_outputs.iter().next().copied().unwrap();
    assert_eq!(payout.owner(), owner);
    assert_eq!(payout.delegate(), Some(maker));
    assert_eq!(payout.value(), input.timeout_payout);

    alice
        .configure_paid_serve(
            revision + 1,
            Some(PaidServeInput {
                policy: policy.clone(),
                routes: vec![PaidRouteInput {
                    peer: input.client_peer.clone(),
                    client: bst.identity.node_key,
                    bond: offered.bond.clone(),
                }],
            }),
        )
        .await
        .unwrap();
    alice.set_provider(true).await.unwrap();
    let pool = PaidPoolInput {
        id: "real".into(),
        providers: vec![PaidProviderInput {
            provider: ast.identity.transport_id.clone(),
            addresses: input.addresses,
            trust_policy: request.client.trust_policy.clone(),
            bond: offered.bond,
            coins: vec![hex::encode(payment.to_bytes())],
            omission_bond: 6001,
            acceptance_blocks: 256,
            terminal_blocks: 1024,
            payment_blocks: 512,
            timeout_seconds: 60,
            policy: policy.clone(),
        }],
        max_pending_requests: 8,
    };
    bob.save_paid_pool(pool.clone()).await.unwrap();
    request.client.target = WorkTarget::Paid {
        pool_id: "real".into(),
        provider: ast.identity.transport_id,
        service: policy.service.clone(),
        method: policy.method.clone(),
    };
    let args = crate::paid::pool_options(b.path(), &pool)
        .unwrap()
        .providers
        .remove(0);
    let endpoint = hellas_sdk::paid_client::bind_paid_endpoint(
        bl.clients.paid_transport(&bl, "real").await.unwrap(),
    )
    .await
    .unwrap();
    let signer = hellas_sdk::kernel::Secp256k1Signer::from_secret_scalar(
        bl.identity.node.node_secret_bytes(),
    )
    .unwrap();
    let mut session = sealing_while(
        &mut devnet,
        Box::pin(hellas_sdk::paid_client::PaidWorkSession::open(
            args,
            endpoint.clone(),
            signer,
            client_node,
            bl.coordinator.clone(),
        )),
    )
    .await
    .unwrap();
    session.shutdown().await;
    drop(session);
    endpoint.close().await;
    let events = std::sync::Mutex::new(vec![]);
    let id = crate::run::execute(&bob, request.clone(), |event| {
        events.lock().unwrap().push(event)
    })
    .await
    .unwrap();
    let history = bob.history.list(512).unwrap();
    assert_eq!(history.len(), 1, "the funded Run is issued once");
    let record = history.into_iter().find(|entry| entry.id == id).unwrap();
    assert_eq!(record.status, "complete", "{}", record.error);
    assert_eq!(
        origin.as_ref().map_or_else(
            || upstream.calls.load(Ordering::SeqCst),
            |origin| origin.calls.load(Ordering::SeqCst)
        ),
        1
    );
    let events = events.into_inner().unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ExecutionEvent::Verification { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event,ExecutionEvent::Finished{run_id} if run_id==&id))
    );
    let (key, bearer) = bob
        .issue_gateway_key(GatewayKeyInput {
            request_id: hex::encode([234; 16]),
            label: "paid HTTP".into(),
            period: crate::period::Period::Week,
        })
        .await
        .unwrap();
    let config = GatewayConfig {
        client: request.client.clone(),
        http_routes: if responses {
            vec![]
        } else {
            vec![HttpRouteInput {
                path: "/proxy".into(),
                method: "GET".into(),
                url: format!("{}/", upstream.url),
                credential: None,
                max_response_bytes: 4096,
                tls_roots: vec![upstream.root.clone()],
            }]
        },
        port: Some(0),
        zdr: true,
    };
    bob.set_gateway(true, Some(config.clone())).await.unwrap();
    let address = bob.gateway_address().await.unwrap();
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let path = if responses { "/v1/responses" } else { "/proxy" };
    let url = format!("{address}{path}");
    let calls = origin.as_ref().map_or_else(
        || upstream.calls.load(Ordering::SeqCst),
        |o| o.calls.load(Ordering::SeqCst),
    );
    for token in [None, Some("invalid")] {
        let mut req = gateway_request(&http, &url, &request, responses);
        if let Some(token) = token {
            req = req.bearer_auth(token);
        }
        assert_eq!(req.send().await.unwrap().status(), 401);
    }
    assert_eq!(
        origin.as_ref().map_or_else(
            || upstream.calls.load(Ordering::SeqCst),
            |o| o.calls.load(Ordering::SeqCst)
        ),
        calls
    );
    completed_gateway_response(&http, &url, &request, responses, &bearer).await;
    assert_eq!(
        origin.as_ref().map_or_else(
            || upstream.calls.load(Ordering::SeqCst),
            |origin| origin.calls.load(Ordering::SeqCst)
        ),
        calls + 1
    );
    assert!(!b.path().join("gateway-archive").exists());
    if !responses {
        let original = bl.clients.target(&bl, &request.client).await.unwrap();
        let saved = std::fs::read(b.path().join("gateway-configuration.json")).unwrap();
        upstream.hold.store(true, Ordering::SeqCst);
        upstream.started.notified().await;
        let held_http = http.clone();
        let held_url = url.clone();
        let held_request = request.clone();
        let held_bearer = bearer.clone();
        let pending = tokio::spawn(async move {
            completed_gateway_response(&held_http, &held_url, &held_request, false, &held_bearer)
                .await;
        });
        tokio::time::timeout(Duration::from_secs(120), upstream.started.notified())
            .await
            .unwrap();
        let mut bad_path = config.clone();
        bad_path.http_routes[0].path = "/proxy?invalid".into();
        let mut duplicate = config.clone();
        duplicate.http_routes.push(duplicate.http_routes[0].clone());
        for candidate in [bad_path, duplicate] {
            let mut desired = desired_https(bob.provider_view().await.unwrap());
            desired.gateway = Some(candidate.clone());
            assert!(bob.preview_apply(desired).await.is_err());
            let refusal = tokio::time::timeout(
                Duration::from_secs(5),
                bob.set_gateway(true, Some(candidate)),
            )
            .await
            .expect("route validation must not wait for accepted work to drain")
            .unwrap_err();
            assert!(
                matches!(
                    refusal.downcast_ref::<hellas_gateway::GatewayError>(),
                    Some(hellas_gateway::GatewayError::Config(
                        hellas_gateway::GatewayConfigError::RoutePath
                            | hellas_gateway::GatewayConfigError::DuplicateRoute
                    ))
                ),
                "{refusal:#}"
            );
            assert_eq!(
                std::fs::read(b.path().join("gateway-configuration.json")).unwrap(),
                saved
            );
            assert_eq!(bob.gateway_address().await.unwrap(), address);
            assert_eq!(bob.status().await.unwrap().gateway.state, "running");
            assert!(Arc::ptr_eq(
                &original,
                &bl.clients.target(&bl, &request.client).await.unwrap()
            ));
            assert!(!pending.is_finished());
        }
        upstream.release.notify_one();
        pending.await.unwrap();
        upstream.hold.store(false, Ordering::SeqCst);
    }
    let mut archived = config;
    archived.zdr = false;
    bob.set_gateway(true, Some(archived)).await.unwrap();
    let url = format!("{}{path}", bob.gateway_address().await.unwrap());
    completed_gateway_response(&http, &url, &request, responses, &bearer).await;
    assert_eq!(
        origin.as_ref().map_or_else(
            || upstream.calls.load(Ordering::SeqCst),
            |origin| origin.calls.load(Ordering::SeqCst)
        ),
        calls + 2 + usize::from(!responses)
    );
    assert!(
        std::fs::read_dir(b.path().join("gateway-archive"))
            .unwrap()
            .any(|e| e.unwrap().path().join("response.bin").exists())
    );
    bob.revoke_gateway_key(&key.id).await.unwrap();
    assert_eq!(
        gateway_request(&http, &url, &request, responses)
            .bearer_auth(&bearer)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    bob.shutdown().await.unwrap();
    assert!(
        gateway_request(&http, &url, &request, responses)
            .bearer_auth(&bearer)
            .send()
            .await
            .is_err()
    );
    alice.shutdown().await.unwrap();
    devnet.shutdown().await;
}

fn gateway_request(
    http: &reqwest::Client,
    url: &str,
    request: &RunRequest,
    responses: bool,
) -> reqwest::RequestBuilder {
    if responses {
        http.post(url)
            .header("content-type", "application/json")
            .body(request.input.clone())
    } else {
        http.get(url)
    }
}
#[test]
fn paid_preview_provision_and_http_work_use_real_validators_and_delegated_coins() {
    run(|runtime| funded_gateway(runtime, false));
}
#[test]
fn paid_responses_gateway_uses_real_validators_and_delegated_coins() {
    run(|runtime| funded_gateway(runtime, true));
}

async fn completed_gateway_response(
    http: &reqwest::Client,
    url: &str,
    request: &RunRequest,
    responses: bool,
    bearer: &str,
) {
    tokio::time::timeout(Duration::from_secs(120), async {
        let response = gateway_request(http, url, request, responses)
            .bearer_auth(bearer)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(response.text().await.unwrap().contains("hello"));
    })
    .await
    .expect("a full verified response, not only HTTP headers");
}
