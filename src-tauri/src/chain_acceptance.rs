//! Funded Gate acceptance using the SDK's six independently executing validators.
//! The SDK fixture orders blocks; kernel execution, signatures and transports are real.
use super::*;
use hellas_chain::{
    LightClient as _,
    test_support::devnet::{Devnet, DevnetConfig, GenesisOwner, OwnerKind, run},
};
use hellas_gateway::WorkExecutionBackend as _;
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
        compute_credit_limit: 40,
        delivery_credit_limit: 40,
        fixed_price: 10,
        dispatch_blocks: 16,
        delivery_blocks: 16,
        oracle_blocks: 16,
        jobs_per_channel: 10,
        omission_bond: 101,
        max_stake_per_client: 10000,
        max_open_offers: 2,
        bond_lifetime_blocks: 2000,
        redelegate_payout: true,
        limits: paid_limits(),
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
    use hellas_authority::authenticator::kernel_auth;
    use hellas_sdk::coin_owner::OwnerReshape;
    let config = crate::paid::config(
        &policy.chain,
        policy.poll_ms,
        policy.max_observation_age_ms,
        local.directory.join("unused-plan"),
    )
    .unwrap();
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
        None,
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

struct FundedGateway {
    directories: [tempfile::TempDir; 4],
    devnet: Devnet,
    alice: AppState,
    bob: AppState,
    bl: Arc<crate::control::LocalNode>,
    upstream: Upstream,
    origin: Option<crate::responses_fixture::Origin>,
    request: RunRequest,
    responses: bool,
}

fn funded_gateway(
    runtime: commonware_runtime::tokio::Context,
    responses: bool,
) -> impl std::future::Future<Output = ()> {
    Box::pin(async move {
        let _network_slot = CHAIN_TEST.lock().await;
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                "hellas_sdk=info,hellas_gateway=debug,hellas_work=info,hellas_chain=warn",
            )
            .with_ansi(false)
            .try_init();
        // Keep the large setup and listener futures off the test thread's stack.
        let case = Box::pin(funded_setup(runtime, responses)).await;
        Box::pin(funded_listener(case)).await;
    })
}

async fn funded_setup(
    runtime: commonware_runtime::tokio::Context,
    responses: bool,
) -> FundedGateway {
    const ALICE: GenesisOwner = GenesisOwner {
        kind: OwnerKind::Passkey,
        secret_scalar: [11; 32],
        balance: 10000,
    };
    const BOB: GenesisOwner = GenesisOwner {
        kind: OwnerKind::Passkey,
        secret_scalar: [13; 32],
        balance: 10000,
    };
    const NATIVE: GenesisOwner = GenesisOwner {
        kind: OwnerKind::Native,
        secret_scalar: [83; 32],
        balance: 12,
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
    assert_eq!(
        ast.identity.principal,
        format!("p256:{}", hex::encode(ALICE.key().to_bytes()))
    );
    assert_eq!(
        bst.identity.principal,
        format!("p256:{}", hex::encode(BOB.key().to_bytes()))
    );
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
    let native_coin =
        sealing_while(&mut devnet, Box::pin(delegate(&al, &policy, Some(&native)))).await;
    eprintln!(
        "real-chain fixture: native and passkey owner delegations finalized through the same SDK"
    );
    alice
        .configure_paid_serve(
            revision,
            Some(PaidServeInput {
                policy: policy.clone(),
            }),
        )
        .await
        .unwrap();
    let approved = al.coordinator.paid_policy().await.unwrap().unwrap();
    assert_eq!(
        approved.policy,
        crate::paid::standing_policy(&policy).unwrap()
    );
    assert_eq!(approved.revision.0, 1);
    let addresses = ast
        .bound_addresses
        .iter()
        .filter_map(|s| s.parse::<std::net::SocketAddr>().ok())
        .filter(|a| a.is_ipv4())
        .map(|a| format!("127.0.0.1:{}", a.port()))
        .collect();
    let mut pool = PaidPoolInput {
        id: "real".into(),
        providers: vec![PaidProviderInput {
            provider: alice.status().await.unwrap().identity.provider_address,
            addresses,
            trust_policy: request.client.trust_policy.clone(),
            acceptance_blocks: 256,
            terminal_blocks: 1024,
            payment_blocks: 512,
            timeout_seconds: 60,
            policy: client_policy(&policy),
        }],
        max_pending_requests: 8,
    };
    bob.save_paid_pool(pool.clone()).await.unwrap();
    request.client.target = WorkTarget::Paid {
        pool_id: "real".into(),
        provider: pool.providers[0].provider.clone(),
        service: policy.service.clone(),
        method: policy.method.clone(),
    };
    let args = crate::paid::pool_options(b.path(), &pool)
        .unwrap()
        .providers
        .remove(0);
    let buyer_before = devnet
        .full_node(1)
        .view()
        .unwrap()
        .client()
        .get_coins_by_owner(BOB.key())
        .await
        .unwrap()
        .unwrap()
        .coins;
    // A concurrent approval replacement cannot relabel the captured quote trust.
    let old = trust::list(b.path())
        .unwrap()
        .into_iter()
        .find(|record| record.id == request.client.trust_policy)
        .unwrap();
    let resolved = Arc::new(Notify::new());
    let resume = Arc::new(Notify::new());
    *bl.clients.after_paid_resolution.lock().unwrap() = Some((resolved.clone(), resume.clone()));
    let opening = bl.clone();
    let selected = request.client.clone();
    let pending = tokio::spawn(async move { opening.clients.target(&opening, &selected).await });
    tokio::time::timeout(std::time::Duration::from_secs(10), resolved.notified())
        .await
        .unwrap();
    let replacement = |revision, genesis: String| TrustInput {
        id: old.id.clone(),
        expected_revision: revision,
        genesis,
        apple: false,
        app_id: String::new(),
        cd_hashes: vec![],
        reason: "Independent replacement during quote".into(),
    };
    trust::approve(b.path(), replacement(old.revision, hex::encode([246; 32]))).unwrap();
    resume.notify_one();
    let first = sealing_while(&mut devnet, pending).await.unwrap().unwrap();
    assert_eq!(
        bl.clients.opened_paid_trust.lock().unwrap()[0][0]
            .expected_genesis
            .to_string(),
        old.genesis
    );
    let refusal = bl.clients.target(&bl, &request.client).await.err().unwrap();
    assert!(
        refusal.to_string().contains("approved enrollment"),
        "{refusal:#}"
    );
    assert_eq!(bl.clients.opened_paid_trust.lock().unwrap().len(), 1);
    trust::approve(b.path(), replacement(old.revision + 1, old.genesis.clone())).unwrap();
    let target = sealing_while(&mut devnet, bl.clients.target(&bl, &request.client))
        .await
        .unwrap();
    assert!(
        !Arc::ptr_eq(&first, &target),
        "a new approval must reopen the pool"
    );
    assert_eq!(bl.clients.opened_paid_trust.lock().unwrap().len(), 2);
    assert!(Arc::ptr_eq(
        &target,
        &bl.clients.target(&bl, &request.client).await.unwrap()
    ));
    drop(first);
    let discovery =
        hellas_channel::work_store::discover_setups(&args.journal_root, args.config.chain.network)
            .await
            .unwrap();
    assert!(
        discovery.setups.is_empty() && discovery.unidentified.is_empty(),
        "quoting must not create a channel"
    );
    let provider_discovery = hellas_channel::work_store::discover_setups(
        &a.path().join("paid-provider"),
        args.config.chain.network,
    )
    .await
    .unwrap();
    assert!(provider_discovery.setups.is_empty() && provider_discovery.unidentified.is_empty());
    assert_eq!(
        devnet
            .full_node(1)
            .view()
            .unwrap()
            .client()
            .get_coins_by_owner(BOB.key())
            .await
            .unwrap()
            .unwrap()
            .coins,
        buyer_before,
        "quote discovery must not spend buyer funding"
    );
    let owner = bl.clients.funding_owner(&bl).await.unwrap();
    let signer = hellas_sdk::kernel::Secp256k1Signer::from_secret_scalar(
        bl.identity.node.node_secret_bytes(),
    )
    .unwrap();
    let events = std::sync::Mutex::new(vec![]);
    let id = sealing_while(
        &mut devnet,
        crate::run::execute(&bob, request.clone(), |event| {
            events.lock().unwrap().push(event)
        }),
    )
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
    pool.providers[0].timeout_seconds = 90;
    bob.save_paid_pool(pool.clone()).await.unwrap();
    assert!(
        target
            .backend
            .fetch(hellas_gateway::WorkFetchRequest {
                provider: target.provider,
                resource: target.resource,
                channel: target.channel,
                service: target.service.clone(),
                method: target.method.clone(),
                body: request.input.as_bytes().to_vec()
            })
            .is_err(),
        "a saved pool edit must retire the old target"
    );
    let replacement = sealing_while(&mut devnet, bl.clients.target(&bl, &request.client))
        .await
        .unwrap();
    assert!(!Arc::ptr_eq(&target, &replacement));
    assert_eq!(replacement.backend.timeout(), Some(Duration::from_secs(90)));
    bl.clients.reset().await.unwrap();
    drop(replacement);
    drop(target);
    let endpoint = hellas_sdk::paid_client::bind_paid_endpoint(
        bl.clients.paid_transport(&bl, "real").await.unwrap(),
    )
    .await
    .unwrap();
    let mut session = sealing_while(
        &mut devnet,
        hellas_sdk::paid_client::PaidWorkSession::open(
            args.clone(),
            endpoint.clone(),
            signer.clone(),
            owner.clone(),
            hellas_sdk::paid_client::SessionOpenMode::Retained,
            Default::default(),
        ),
    )
    .await
    .unwrap();
    let descriptor = session.descriptor();
    let bond = descriptor.bond_edge();
    let terms = &descriptor.channel().payment_terms().bond_terms;
    let maker = hellas_sdk::kernel::Secp256k1Signer::from_secret_scalar(
        al.identity.node.node_secret_bytes(),
    )
    .unwrap()
    .party_key();
    assert_eq!(terms.parties.maker().owner(), ALICE.key().into());
    assert_eq!(terms.parties.maker().signer(), maker);
    assert_eq!(terms.parties.maker().payout_delegate(), Some(maker));
    assert_eq!(terms.parties.taker().owner(), BOB.key().into());
    assert_eq!(
        terms.timeout_outputs.iter().next().unwrap().value(),
        ALICE.balance
    );
    assert_eq!(descriptor.payment_policy().fixed_price, policy.fixed_price);
    assert_eq!(
        descriptor.channel().payment_terms().omission_bond,
        policy.omission_bond
    );
    let completed_terminal = session
        .with_state(|state| {
            assert_eq!(state.jobs().len(), 0);
            assert_eq!(state.terminals().len(), 1);
            assert_eq!(state.proposal_nonce_high_water(), 1);
            assert_eq!(state.ledger().credited_cumulative(), policy.fixed_price);
            let terminal = state.terminals().next().unwrap();
            assert!(matches!(
                terminal.outcome,
                hellas_channel::work_store::TerminalOutcome::Certified { .. }
            ));
            terminal.clone()
        })
        .await
        .unwrap();
    session.shutdown().await;
    drop(session);
    let mut reopened = sealing_while(
        &mut devnet,
        hellas_sdk::paid_client::PaidWorkSession::open(
            args.clone(),
            endpoint.clone(),
            signer,
            owner,
            hellas_sdk::paid_client::SessionOpenMode::Retained,
            Default::default(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        reopened.descriptor().bond_edge(),
        bond,
        "retained reopening must not fund a second channel"
    );
    reopened
        .with_state(|state| {
            assert_eq!(state.jobs().len(), 0);
            assert_eq!(state.terminals().len(), 1);
            assert_eq!(state.proposal_nonce_high_water(), 1);
            assert_eq!(state.ledger().credited_cumulative(), policy.fixed_price);
            assert_eq!(state.terminals().next().unwrap(), &completed_terminal);
        })
        .await
        .unwrap();
    reopened.shutdown().await;
    drop(reopened);
    endpoint.close().await;
    let discovery =
        hellas_channel::work_store::discover_setups(&args.journal_root, args.config.chain.network)
            .await
            .unwrap();
    assert_eq!(discovery.setups.len(), 1);
    assert!(discovery.unidentified.is_empty());
    let snapshot = devnet
        .full_node(1)
        .view()
        .unwrap()
        .client()
        .get_coins_by_owner(NATIVE.key())
        .await
        .unwrap()
        .unwrap();
    assert!(
        snapshot
            .coins
            .iter()
            .any(|(id, _)| id.0 == native_coin.to_bytes()),
        "another owner's delegated coin must remain unspent"
    );
    assert!(
        devnet
            .full_node(0)
            .view()
            .unwrap()
            .client()
            .get_coins_by_owner(ALICE.key())
            .await
            .unwrap()
            .unwrap()
            .coins
            .iter()
            .all(|(id, _)| id.0 != stake.to_bytes())
    );
    assert_eq!(
        origin.as_ref().map_or_else(
            || upstream.calls.load(Ordering::SeqCst),
            |origin| origin.calls.load(Ordering::SeqCst)
        ),
        1,
        "reopening must not execute the request again"
    );
    eprintln!("real-chain fixture: funded Run and retained payment verified");
    FundedGateway {
        directories: [a, b, sa, sb],
        devnet,
        alice,
        bob,
        bl,
        upstream,
        origin,
        request,
        responses,
    }
}

async fn funded_listener(case: FundedGateway) {
    let FundedGateway {
        directories: [_a, b, _sa, _sb],
        devnet,
        alice,
        bob,
        bl,
        upstream,
        origin,
        request,
        responses,
    } = case;
    eprintln!("real-chain fixture: checking paid listener authentication and delivery");
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
fn paid_automatic_offers_and_http_work_use_real_validators_and_owner_funding() {
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
