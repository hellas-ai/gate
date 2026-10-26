//! A local six-validator network. Authentication, execution and settlement are SDK paths.
use super::*;
use commonware_codec::{DecodeExt as _, Encode as _};
use commonware_cryptography::Digestible as _;
use hellas_chain::LightClient as _;
use hellas_sdk::kernel::{CoinId, Key};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

#[test]
#[ignore = "subprocess entry point used by the real-chain acceptance test"]
fn validator_process() {
    let config = std::env::var_os("GATE_TEST_VALIDATOR_CONFIG").expect("fixture subprocess only");
    hellas_chain::validator::run_command(hellas_chain::validator::Command::Run {
        config: config.into(),
    })
    .unwrap();
}
struct Validators {
    children: Vec<Child>,
    cadence: Option<std::thread::JoinHandle<()>>,
    running: Arc<AtomicBool>,
}
impl Validators {
    fn pace(&mut self) {
        let pids: Vec<_> = self
            .children
            .iter()
            .skip(2)
            .map(|child| libc::pid_t::try_from(child.id()).unwrap())
            .collect();
        let running = self.running.clone();
        // Control block cadence, as a network fixture. Validators still agree,
        // execute and sign every block; full nodes execute certified history.
        // Pause a quorum; two validators keep RPC and execution alive so full
        // nodes can observe a stable finalized frontier between consensus bursts.
        self.cadence = Some(std::thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                for pid in &pids {
                    unsafe {
                        libc::kill(*pid, libc::SIGCONT);
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
                for pid in &pids {
                    unsafe {
                        libc::kill(*pid, libc::SIGSTOP);
                    }
                }
                std::thread::sleep(Duration::from_secs(3));
            }
            for pid in &pids {
                unsafe {
                    libc::kill(*pid, libc::SIGCONT);
                }
            }
        }));
    }
}
impl Drop for Validators {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(cadence) = self.cadence.take() {
            cadence.join().unwrap();
        }
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn free_ports(count: u16) -> u16 {
    loop {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let start = probe.local_addr().unwrap().port();
        if start > 65535 - count {
            continue;
        }
        let sockets = (start + 1..start + count)
            .map(|p| std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, p)))
            .collect::<Result<Vec<_>, _>>();
        if sockets.is_ok() {
            return start;
        }
    }
}
async fn network(directory: &Path, owners: &[Key]) -> (Validators, PaidPolicyInput) {
    let start = free_ports(6);
    let rpc = free_ports(6);
    let config_dir = directory.join("validators");
    hellas_chain::validator::run_command(hellas_chain::validator::Command::GenerateNetwork {
        network_id: "gate-paid-acceptance".into(),
        validators: 6,
        labels: (0..6).map(|i| format!("validator-{i}")).collect(),
        addresses: vec!["127.0.0.1".into(); 6],
        start_port: start,
        metrics_base_port: free_ports(6),
        relay_urls: vec![],
        genesis_allocations: owners
            .iter()
            .map(|k| format!("{}:10000", hellas_chain::domain::SettlementKey::from(*k)))
            .collect(),
        treasury_balance: None,
        output_dir: config_dir.clone(),
    })
    .unwrap();
    let mut processes = Validators {
        children: vec![],
        cadence: None,
        running: Arc::new(AtomicBool::new(true)),
    };
    let mut policy = paid_policy_fixture(directory).await;
    let config_path = config_dir.join("validator-0.toml");
    let config: hellas_chain::config::ValidatorConfig =
        toml::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    let genesis = config.genesis.clone();
    let scheme = hellas_chain::domain::Scheme::signer(
        &hellas_chain::consensus_namespace(
            config.network_id().unwrap(),
            hellas_chain::genesis_digest(&genesis),
        ),
        config.participants().unwrap(),
        config.decode_threshold_polynomial().unwrap(),
        config.decode_threshold_share().unwrap(),
    )
    .unwrap();
    let leader = genesis
        .validators
        .iter()
        .map(|v| {
            hellas_chain::domain::PublicKey::decode(hex::decode(&v.public_key).unwrap().as_slice())
                .unwrap()
        })
        .min()
        .unwrap();
    let network = config.network_id().unwrap();
    let document = hellas_chain::genesis_digest(&genesis);
    let probe_dir = directory.join("live-genesis");
    let digest = tokio::task::spawn_blocking(move || {
        use commonware_runtime::Runner as _;
        commonware_runtime::tokio::Runner::new(
            commonware_runtime::tokio::Config::new().with_storage_directory(probe_dir),
        )
        .start(move |context| async move {
            hellas_chain::Application::new(
                context,
                network,
                document,
                leader,
                vec![],
                "probe",
                Default::default(),
            )
            .await
            .genesis_block()
            .digest()
        })
    })
    .await
    .unwrap();
    policy.chain = ChainInput {
        network: genesis.network_id,
        genesis_digest: hellas_auth::Digest::from(digest.0).to_string(),
        threshold_identity: hex::encode(scheme.identity().encode()),
        validators: (rpc..rpc + 6)
            .map(|port| format!(" \tWS://127.0.0.1:{port}\n"))
            .collect(),
        committee: genesis
            .validators
            .into_iter()
            .map(|v| GenesisValidatorInput {
                public_key: v.public_key,
                label: v.label,
            })
            .collect(),
        allocations: genesis
            .allocations
            .into_iter()
            .map(|a| GenesisAllocationInput {
                address: a.address,
                balance: a.balance,
            })
            .collect(),
    };
    policy.service = "web".into();
    policy.method = "get".into();
    policy.presentation = FetchPresentation::Http;
    policy.dispatch_blocks = 16;
    policy.delivery_blocks = 16;
    policy.oracle_blocks = 16;
    policy.value = 10000;
    policy.reserve = 0;
    policy.poll_ms = 50;
    policy.max_observation_age_ms = 10000;
    for i in 0..6u16 {
        let path = config_dir.join(format!("validator-{i}.toml"));
        let mut config: hellas_chain::config::ValidatorConfig =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        config.metrics_port = None;
        config.light_client_bind = Some(([127, 0, 0, 1], rpc + i).into());
        std::fs::write(&path, toml::to_string_pretty(&config).unwrap()).unwrap();
        let output = std::fs::File::create(directory.join(format!("validator-{i}.log"))).unwrap();
        processes.children.push(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "integration_tests::chain_acceptance::validator_process",
                    "--ignored",
                    "--nocapture",
                ])
                .env("GATE_TEST_VALIDATOR_CONFIG", path)
                .env("XDG_DATA_HOME", directory.join("chain-data"))
                .env_remove("CREDENTIALS_DIRECTORY")
                .stdout(output.try_clone().unwrap())
                .stderr(output)
                .stdin(Stdio::null())
                .spawn()
                .unwrap(),
        );
    }
    processes.pace();
    (processes, policy)
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
    let plan = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match OwnerReshape::prepare(
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
            {
                Ok(plan) => break plan,
                Err(hellas_sdk::coin_owner::CoinOwnerError::Query(
                    hellas_chain::QueryError::StateUnavailable(_),
                )) => tokio::time::sleep(Duration::from_millis(50)).await,
                Err(error) => panic!("owner delegation: {error:?}"),
            }
        }
    })
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

async fn funded_gateway(responses: bool) {
    let _network_slot = CHAIN_TEST.lock().await;
    let _ = tracing_subscriber::fmt()
        .with_env_filter("hellas_sdk=info,hellas_gateway=debug,hellas_work=info,hellas_chain=warn")
        .with_ansi(false)
        .try_init();
    let directory_path = tempfile::tempdir().unwrap().keep();
    let directory = directory_path.as_path();
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
    let ast = alice.initialize_software("Alice".into()).await.unwrap();
    let bst = bob.initialize_software("Bob".into()).await.unwrap();
    let al = alice.local().await.unwrap();
    let bl = bob.local().await.unwrap();
    let native = hellas_sdk::kernel::Secp256k1Signer::from_secret_scalar([83; 32]).unwrap();
    let (_validators, mut policy) = network(
        directory,
        &[
            Key::from_bytes(*al.coordinator.owner().await.unwrap().as_bytes()),
            Key::from_bytes(*bl.coordinator.owner().await.unwrap().as_bytes()),
            native.party_key(),
        ],
    )
    .await;
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
    let stake = delegate(&al, &policy, None).await;
    let payment = delegate(&bl, &policy, None).await;
    let native_coin = delegate(&bl, &policy, Some(&native)).await;
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
    let node = al
        .chain
        .get(&crate::paid::config(&policy, a.path().join("unused")).unwrap())
        .await
        .unwrap();
    let height = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Ok(view) = node.view() {
                match view.client().get_latest_block().await {
                    Ok(Some(snapshot)) => break snapshot.height,
                    Ok(None) | Err(hellas_chain::QueryError::StateUnavailable(_)) => {}
                    Err(error) => panic!("finalized tip: {error:?}"),
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
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
    let refused = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match alice.provision_paid(wrong.clone(), true).await {
                Err(error) if error.to_string().contains("catching up") => {
                    tokio::time::sleep(Duration::from_millis(50)).await
                }
                result => break result,
            }
        }
    })
    .await
    .unwrap()
    .unwrap_err();
    assert!(
        matches!(refused.downcast_ref::<hellas_sdk::coin_owner::CoinOwnerError>(),Some(hellas_sdk::coin_owner::CoinOwnerError::Missing(id)) if *id==native_coin),
        "{refused:#}"
    );
    assert!(!a.path().join("paid-provider").exists());
    let offered = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match alice.provision_paid(input.clone(), true).await {
                Ok(offer) => break offer,
                Err(error) if error.to_string().contains("catching up") => {
                    tokio::time::sleep(Duration::from_millis(50)).await
                }
                Err(error) => panic!("provision: {error:#}"),
            }
        }
    })
    .await
    .unwrap();
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
    bob.save_paid_pool(PaidPoolInput {
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
    })
    .await
    .unwrap();
    request.client.target = WorkTarget::Paid {
        pool_id: "real".into(),
        provider: ast.identity.transport_id,
        service: policy.service.clone(),
        method: policy.method.clone(),
    };
    tokio::time::timeout(Duration::from_secs(240), async {
        loop {
            let events = std::sync::Mutex::new(vec![]);
            let id = crate::run::execute(&bob, request.clone(), |event| {
                events.lock().unwrap().push(event)
            })
            .await
            .unwrap();
            let record = bob
                .history
                .list(512)
                .unwrap()
                .into_iter()
                .find(|entry| entry.id == id)
                .unwrap();
            let events = events.into_inner().unwrap();
            if record.status == "complete" {
                assert!(
                    events
                        .iter()
                        .any(|event| matches!(event, ExecutionEvent::Verification { .. }))
                );
                assert!(
                    events.iter().any(
                        |event| matches!(event,ExecutionEvent::Finished{run_id} if run_id==&id)
                    )
                );
                break;
            }
            assert_eq!(record.status, "failed");
            assert!(!events.iter().any(|event| matches!(
                event,
                ExecutionEvent::Verification { .. } | ExecutionEvent::Finished { .. }
            )));
            let error = record.error;
            assert!(
                error.contains("no eligible paid provider")
                    || error.contains("refused as not ready"),
                "paid Run: {error}"
            );
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
    .await
    .unwrap();
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
#[tokio::test(flavor = "multi_thread")]
async fn paid_preview_provision_and_http_work_use_real_validators_and_delegated_coins() {
    funded_gateway(false).await;
}
#[tokio::test(flavor = "multi_thread")]
async fn paid_responses_gateway_uses_real_validators_and_delegated_coins() {
    funded_gateway(true).await;
}

async fn completed_gateway_response(
    http: &reqwest::Client,
    url: &str,
    request: &RunRequest,
    responses: bool,
    bearer: &str,
) {
    tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let response = gateway_request(http, url, request, responses)
                .bearer_auth(bearer)
                .send()
                .await
                .unwrap();
            let status = response.status();
            match response.text().await {
                Ok(body) if status == 200 && body.contains("hello") => break,
                Err(error) if status == 200 && error.is_decode() => {} // An admission refusal after streaming headers must not count as completion.
                Ok(body) if status == 503 || status == 502 => {
                    eprintln!("paid gateway awaiting admission: {status}: {body}")
                }
                other => panic!("paid gateway response {status}: {other:?}"),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
    .await
    .expect("a full verified response, not only HTTP headers");
}
