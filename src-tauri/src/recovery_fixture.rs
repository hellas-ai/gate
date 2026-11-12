//! A previously accepted retained Compute job for host recovery tests.
//! Gate's new Fetch jobs are ephemeral; this journal exercises recovery of older
//! retained work through the canonical setup and channel stores. Its signed
//! setup is a storage fixture, not a transaction submitted to the test chain.
use hellas_channel::{
    model::{
        bundle::WorkChannelSetupBundleV2,
        offer::{PaidOffer, SignedPaidOffer},
        setup::ProviderChannelPolicy,
        work::{private_policy_commitment, signing_hash, work_id},
    },
    test_support::channel as fixture,
    work_store::{ChannelRecord, ChannelStore, Role, SetupRecord, SetupStore},
};
use hellas_sdk::{kernel::*, work_config::WorkConfig};
use hellas_store::content::{ContentStore as _, DiskStore, PinName, PinValue, Scope};
use std::collections::BTreeMap;

pub struct Retained {
    prefix: String,
    pins: BTreeMap<PinName, PinValue>,
    input: Vec<u8>,
}

impl Retained {
    pub async fn seed(local: &crate::control::LocalNode, config: &WorkConfig) -> Self {
        let provider =
            Secp256k1Signer::from_secret_scalar(local.identity.node.node_secret_bytes()).unwrap();
        let client = fixture::client();
        let network = config.network();
        let funding = |byte| {
            Funding::new(
                hellas_sdk::work_config::coin_list(&[CoinId::from_bytes([byte; CoinId::LENGTH])])
                    .unwrap(),
                hellas_sdk::work_config::coin_list(&[]).unwrap(),
            )
        };
        let bond_funding = funding(0xe1);
        let payment_funding = funding(0xe2);
        let bond = fixture::bond_terms_for(
            Parties::new(
                PartyKeys::sole(provider.party_key()),
                PartyKeys::sole(client.party_key()),
            ),
            BlockHeight::new(2000),
            Payout::new(provider.party_key(), 64),
            40,
        );
        let policy = ProviderChannelPolicy {
            network,
            policy_salt: fixture::SALT,
            channel_policy: fixture::credit_policy(40, 40),
            invocation_policy: fixture::work_policy(),
            payment_policy: fixture::payment_policy(),
            expected_payment_values: EdgeValues::new(1000, 200, Fees::ZERO),
        };
        let bond_edge = Tx::edge_id_of(&bond_funding, &Terms::work_stake_bond(bond.clone()));
        let payment = fixture::payment_terms_for(
            bond_edge,
            bond.clone(),
            private_policy_commitment(network, &policy.policy_salt, &policy.channel_policy),
            MAX_START_VALIDITY_BLOCKS,
            601,
        );
        let payment_edge = Tx::edge_id_of(&payment_funding, &Terms::work_payment(payment.clone()));
        let auth = |key: &Secp256k1Signer, hash| PartyAuth::signer(Auth::native(key.sign(hash)));
        let one = WorkChannelSetupBundleV2::propose_bond(
            network,
            bond_funding.clone(),
            bond.clone(),
            auth(
                &provider,
                Tx::open_hash(
                    network,
                    &bond_funding,
                    &Terms::work_stake_bond(bond.clone()),
                ),
            ),
        )
        .unwrap();
        let two = one
            .clone()
            .countersign_bond_and_propose_payment(
                auth(&client, one.bond_open_hash()),
                payment_funding.clone(),
                payment.clone(),
                auth(
                    &client,
                    Tx::open_hash(
                        network,
                        &payment_funding,
                        &Terms::work_payment(payment.clone()),
                    ),
                ),
            )
            .unwrap();
        let three = two
            .clone()
            .countersign_payment(auth(&provider, two.payment_open_hash().unwrap()))
            .unwrap();
        let enrollment = hellas_channel::test_support::enrollment(
            local.identity.node.node_key(),
            local.identity.node.node_key(),
            *local.identity.node.transport_key().public().as_bytes(),
            [3; 32],
        );
        let offer = SignedPaidOffer::sign(
            PaidOffer {
                provider: enrollment,
                proposal: one,
                policy: policy.clone(),
                omission_bond: 601,
                payment_funding,
                addresses: vec![],
            },
            &provider,
        )
        .unwrap();
        let descriptor = policy.describe_close(payment_edge, payment).unwrap();
        let root = config.chain.provider_journals();
        let verifier = Secp256k1Verifier::new();
        let mut setup = SetupStore::open(&root, network, bond_edge, Role::Provider, &verifier)
            .await
            .unwrap();
        for record in [
            SetupRecord::Offer {
                peer: [0x41; 32],
                offer: Box::new(offer),
            },
            SetupRecord::Bundle {
                bundle: two.encode(),
            },
            SetupRecord::ArmedBundle {
                bundle: three.encode(),
                close_descriptor: Box::new(descriptor.clone()),
            },
            SetupRecord::Complete {
                payment_edge,
                origin_height: 0,
                origin_payload: [0; 32],
            },
        ] {
            setup.commit(record, &verifier).await.unwrap();
        }
        let origin = setup.state().origin().unwrap();
        drop(setup);
        let mut journal = ChannelStore::open(
            &root,
            descriptor.channel().clone(),
            descriptor.expected_settlement().unwrap(),
            Role::Provider,
            origin,
            local.content.clone(),
            &verifier,
        )
        .await
        .unwrap();
        let mut prepared = fixture::bundle(0xe3);
        prepared.request.runner_public_key =
            hellas_auth::PublicKey::Secp256k1(provider.party_key().to_bytes());
        let authorization = hellas_channel::job::propose(
            descriptor.channel(),
            &policy.invocation_policy,
            &policy.payment_policy,
            &prepared,
            0xe3,
            fixture::deadlines(),
        )
        .unwrap();
        let work = work_id(descriptor.channel(), &authorization);
        let input = prepared.canonical_bytes().unwrap();
        for record in [
            ChannelRecord::JobProposed {
                authorization,
                client_signature: client.sign(signing_hash(work)),
                prepared_input: input.clone().into(),
                retained_tree: None,
            },
            ChannelRecord::JobAccepted {
                work_id: work,
                provider_signature: provider.sign(signing_hash(work)),
                retained_tree: None,
            },
        ] {
            fixture::commit(&mut journal, record).await;
        }
        drop(journal);
        let prefix = format!("channel/{}/", descriptor.channel().id());
        let pins = hellas_store::content::owner_pins(&local.content, &prefix)
            .await
            .unwrap();
        assert_eq!(pins.len(), 1, "the fixture must retain a real job tree");
        Self {
            prefix,
            pins,
            input,
        }
    }

    pub async fn assert_recovered(&self, store: &DiskStore) {
        assert_eq!(
            hellas_store::content::owner_pins(store, &self.prefix)
                .await
                .unwrap(),
            self.pins
        );
        let bytes = hellas_channel::content::read(
            &hellas_store::content::LocalStore::Disk(store.clone()),
            &Scope::Owner,
            hellas_rpc::ContentRef::for_bytes(&self.input),
            self.input.len() as u64,
        )
        .await
        .unwrap();
        assert_eq!(bytes, self.input);
    }

    /// Leave the signed journal authoritative while its derived pin is absent.
    /// Startup must restore it before unpinned content can be collected.
    pub async fn remove_pins(&self, store: &DiskStore) {
        for (name, value) in &self.pins {
            store
                .set_pin(&Scope::Owner, name, Some(value), None)
                .await
                .unwrap();
        }
        assert!(
            hellas_store::content::owner_pins(store, &self.prefix)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
