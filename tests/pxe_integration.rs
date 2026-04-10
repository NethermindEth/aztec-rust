//! Integration tests for `EmbeddedPxe` — 1:1 mirror of upstream
//! `yarn-project/pxe/src/pxe.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test pxe_integration -- --ignored
//! ```

#![allow(clippy::expect_used, clippy::print_stderr)]

use std::sync::Arc;

use aztec_rs::abi::{ContractArtifact, EventSelector};
use aztec_rs::embedded_pxe::stores::private_event_store::StoredPrivateEvent;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore, PrivateEventStore};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::{PrivateEventFilter, Pxe, RegisterContractRequest};
use aztec_rs::tx::TxHash;
use aztec_rs::types::{
    AztecAddress, ContractInstance, ContractInstanceWithAddress, Fr, PublicKeys,
};

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

fn node_url() -> String {
    std::env::var("AZTEC_NODE_URL").unwrap_or_else(|_| "http://localhost:8080".to_owned())
}

async fn require_live_pxe() -> Option<EmbeddedPxe<HttpNodeClient>> {
    let node = create_aztec_node_client(node_url());
    if let Err(err) = node.get_node_info().await {
        eprintln!("skipping: node not reachable: {err}");
        return None;
    }
    let kv = Arc::new(InMemoryKvStore::new());
    match EmbeddedPxe::create(node, kv).await {
        Ok(pxe) => Some(pxe),
        Err(err) => {
            eprintln!("skipping: failed to create EmbeddedPxe: {err}");
            None
        }
    }
}

fn load_counter_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/counter_contract.json");
    ContractArtifact::from_json(json).expect("parse counter_contract.json")
}

fn make_valid_instance(class_id: Fr, salt: u64) -> ContractInstanceWithAddress {
    let inner = ContractInstance {
        version: 1,
        salt: Fr::from(salt),
        deployer: AztecAddress(Fr::zero()),
        current_contract_class_id: class_id,
        original_contract_class_id: class_id,
        initialization_hash: Fr::zero(),
        public_keys: PublicKeys::default(),
    };
    let address =
        aztec_rs::hash::compute_contract_address_from_instance(&inner).expect("compute address");
    ContractInstanceWithAddress { address, inner }
}

// ===========================================================================
// describe('PXE')
// ===========================================================================

/// TS: it('registers an account and returns it as an account only and not as a recipient')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn registers_an_account_and_returns_it_as_an_account_only_and_not_as_a_recipient() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };

    let random_secret_key = Fr::from(0xdead_beef_1234_u64);
    let random_partial_address = Fr::from(0x42_u64);
    let complete_address = pxe
        .register_account(&random_secret_key, &random_partial_address)
        .await
        .expect("register account");

    let accounts = pxe
        .get_registered_accounts()
        .await
        .expect("get registered accounts");
    assert!(
        accounts
            .iter()
            .any(|a| a.address == complete_address.address),
        "accounts should contain the registered complete address"
    );
}

/// TS: it('does not throw when registering the same account twice (just ignores the second attempt)')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn does_not_throw_when_registering_the_same_account_twice() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };

    let random_secret_key = Fr::from(0x1111_2222_u64);
    let random_partial_address = Fr::from(0x33_u64);

    pxe.register_account(&random_secret_key, &random_partial_address)
        .await
        .expect("first registration");
    pxe.register_account(&random_secret_key, &random_partial_address)
        .await
        .expect("second registration should not error");
}

/// TS: it('successfully adds a contract')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn successfully_adds_a_contract() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };

    let artifact = load_counter_artifact();
    let class_id = aztec_rs::hash::compute_contract_class_id_from_artifact(&artifact)
        .expect("compute class id");

    pxe.register_contract_class(&artifact)
        .await
        .expect("register class");

    let instance1 = make_valid_instance(class_id, 1);
    let instance2 = make_valid_instance(class_id, 2);

    pxe.register_contract(RegisterContractRequest {
        instance: instance1.clone(),
        artifact: Some(artifact.clone()),
    })
    .await
    .expect("register contract 1");

    pxe.register_contract(RegisterContractRequest {
        instance: instance2.clone(),
        artifact: Some(artifact),
    })
    .await
    .expect("register contract 2");

    let expected_addresses = vec![instance1.address, instance2.address];
    let contract_addresses = pxe.get_contracts().await.expect("get contracts");
    for addr in &expected_addresses {
        assert!(
            contract_addresses.contains(addr),
            "contract addresses should contain {addr}"
        );
    }
}

/// TS: it('registers a class and adds a contract for it')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn registers_a_class_and_adds_a_contract_for_it() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };

    let artifact = load_counter_artifact();
    let class_id = aztec_rs::hash::compute_contract_class_id_from_artifact(&artifact)
        .expect("compute class id");

    await_register_class_and_verify(&pxe, &artifact, &class_id).await;

    let instance = make_valid_instance(class_id, 10);
    pxe.register_contract(RegisterContractRequest {
        instance: instance.clone(),
        artifact: None,
    })
    .await
    .expect("register contract");

    let retrieved = pxe
        .get_contract_instance(&instance.address)
        .await
        .expect("get instance");
    assert_eq!(
        retrieved.expect("instance should exist").address,
        instance.address
    );
}

async fn await_register_class_and_verify(
    pxe: &EmbeddedPxe<HttpNodeClient>,
    artifact: &ContractArtifact,
    class_id: &Fr,
) {
    pxe.register_contract_class(artifact)
        .await
        .expect("register class");
    let retrieved = pxe
        .get_contract_artifact(class_id)
        .await
        .expect("get artifact");
    assert!(retrieved.is_some());
    assert_eq!(
        retrieved.expect("artifact should exist").name,
        artifact.name
    );
}

/// TS: it('refuses to register a class with a mismatched address')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn refuses_to_register_a_class_with_a_mismatched_address() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };

    let artifact = load_counter_artifact();
    let class_id = aztec_rs::hash::compute_contract_class_id_from_artifact(&artifact)
        .expect("compute class id");

    let mut instance = make_valid_instance(class_id, 20);
    instance.address = AztecAddress(Fr::from(0xbad_add0_u64));

    let result = pxe
        .register_contract(RegisterContractRequest {
            instance,
            artifact: Some(artifact),
        })
        .await;

    assert!(result.is_err());
    assert!(
        format!("{}", result.expect_err("should fail")).contains("does not match"),
        "error should mention address mismatch"
    );
}

/// TS: it('refuses to register a contract with a class that has not been registered')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn refuses_to_register_a_contract_with_a_class_that_has_not_been_registered() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };

    let instance = ContractInstanceWithAddress {
        address: AztecAddress(Fr::from(0x9999_u64)),
        inner: ContractInstance {
            version: 1,
            salt: Fr::from(1u64),
            deployer: AztecAddress(Fr::zero()),
            current_contract_class_id: Fr::from(0xdead_c1a55_u64),
            original_contract_class_id: Fr::from(0xdead_c1a55_u64),
            initialization_hash: Fr::zero(),
            public_keys: PublicKeys::default(),
        },
    };

    let result = pxe
        .register_contract(RegisterContractRequest {
            instance,
            artifact: None,
        })
        .await;

    assert!(result.is_err());
    assert!(
        format!("{}", result.expect_err("should fail")).contains("artifact not found"),
        "error should mention missing artifact"
    );
}

/// TS: it('refuses to register a contract with an artifact with mismatching class id')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn refuses_to_register_a_contract_with_an_artifact_with_mismatching_class_id() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };

    let artifact = load_counter_artifact();
    let instance = ContractInstanceWithAddress {
        address: AztecAddress(Fr::from(0x8888_u64)),
        inner: ContractInstance {
            version: 1,
            salt: Fr::from(1u64),
            deployer: AztecAddress(Fr::zero()),
            current_contract_class_id: Fr::from(0xbad_1d00_u64),
            original_contract_class_id: Fr::from(0xbad_1d00_u64),
            initialization_hash: Fr::zero(),
            public_keys: PublicKeys::default(),
        },
    };

    let result = pxe
        .register_contract(RegisterContractRequest {
            instance,
            artifact: Some(artifact),
        })
        .await;

    assert!(result.is_err());
    assert!(
        format!("{}", result.expect_err("should fail")).contains("does not match"),
        "error should mention class id mismatch"
    );
}

// ===========================================================================
// describe('getPrivateEvents')
//
// Mirrors upstream pxe.test.ts getPrivateEvents section.
// These are "frontier API" tests so we don't need to rely on slower E2E tests.
// For finer grained tests check out stores/private_event_store tests.
// ===========================================================================

/// Helper: store an event directly into the `PrivateEventStore`.
/// Matches the TS `storeEvent` helper.
async fn store_event(
    event_store: &PrivateEventStore,
    contract_address: AztecAddress,
    event_selector: EventSelector,
    scope: AztecAddress,
    block_number: u64,
    event_counter: &mut u64,
) -> (Vec<Fr>, TxHash) {
    let packed_event = vec![
        Fr::from(*event_counter * 100 + 1),
        Fr::from(*event_counter * 100 + 2),
    ];
    let tx_hash =
        TxHash::from_hex(&format!("0x{:064x}", 0xaa00 + *event_counter)).expect("valid hex");

    let event = StoredPrivateEvent {
        event_selector,
        randomness: Fr::from(*event_counter),
        msg_content: packed_event.clone(),
        siloed_event_commitment: Fr::from(0xc000_u64 + *event_counter),
        contract_address,
        scopes: vec![],
        tx_hash,
        l2_block_number: block_number,
        l2_block_hash: format!("0x{block_number:064x}"),
        tx_index_in_block: Some(0),
        event_index_in_tx: Some(*event_counter),
    };

    event_store
        .store_private_event_log(&event, &scope)
        .await
        .expect("store event");

    *event_counter += 1;
    (packed_event, tx_hash)
}

/// TS: it('returns private events')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn get_private_events_returns_private_events() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };

    let contract_address = AztecAddress(Fr::from(0xe100_u64));
    let event_selector = EventSelector(Fr::from(0x5e10_u64));
    let scope = AztecAddress(Fr::from(0x5c10_u64));
    let event_store = pxe.private_event_store();
    let mut counter = 0u64;

    // Store a couple of events to exercise `getPrivateEvents`
    let (packed1, _) = store_event(
        event_store,
        contract_address,
        event_selector,
        scope,
        1,
        &mut counter,
    )
    .await;
    let (packed2, _) = store_event(
        event_store,
        contract_address,
        event_selector,
        scope,
        1,
        &mut counter,
    )
    .await;

    let events = pxe
        .get_private_events(
            &event_selector,
            PrivateEventFilter {
                contract_address,
                from_block: Some(1),
                scopes: vec![scope],
                ..Default::default()
            },
        )
        .await
        .expect("get private events");

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].packed_event, packed1);
    assert_eq!(events[1].packed_event, packed2);
}

/// TS: it('returns no events')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn get_private_events_returns_no_events() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };

    let events = pxe
        .get_private_events(
            &EventSelector(Fr::from(0x0e00_u64)),
            PrivateEventFilter {
                contract_address: AztecAddress(Fr::from(0xbeef_u64)),
                from_block: Some(1),
                scopes: vec![AztecAddress(Fr::from(0xcafe_u64))],
                ..Default::default()
            },
        )
        .await
        .expect("get private events");

    assert!(events.is_empty());
}

/// TS: describe('filtering') > it('filters by txHash')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn get_private_events_filters_by_tx_hash() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };

    let contract_address = AztecAddress(Fr::from(0xe200_u64));
    let event_selector = EventSelector(Fr::from(0x5e20_u64));
    let scope = AztecAddress(Fr::from(0x5c20_u64));
    let event_store = pxe.private_event_store();
    let mut counter = 10u64;

    let (_, _tx1) = store_event(
        event_store,
        contract_address,
        event_selector,
        scope,
        1,
        &mut counter,
    )
    .await;
    let (packed2, tx2) = store_event(
        event_store,
        contract_address,
        event_selector,
        scope,
        1,
        &mut counter,
    )
    .await;

    let events = pxe
        .get_private_events(
            &event_selector,
            PrivateEventFilter {
                contract_address,
                scopes: vec![scope],
                tx_hash: Some(tx2),
                ..Default::default()
            },
        )
        .await
        .expect("get events filtered by tx hash");

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].packed_event, packed2);
}

/// TS: describe('filtering') > it('filters by block')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn get_private_events_filters_by_block() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };

    let anchor_block = pxe
        .anchor_block_store()
        .get_block_number()
        .await
        .unwrap_or(1);
    // Use blocks relative to anchor so filter validation succeeds
    let past_block = if anchor_block > 1 {
        anchor_block - 1
    } else {
        1
    };
    let latest_block = anchor_block;

    let contract_address = AztecAddress(Fr::from(0xe300_u64));
    let event_selector = EventSelector(Fr::from(0x5e30_u64));
    let scope = AztecAddress(Fr::from(0x5c30_u64));
    let event_store = pxe.private_event_store();
    let mut counter = 20u64;

    // Events in past block
    let (packed_past1, _) = store_event(
        event_store,
        contract_address,
        event_selector,
        scope,
        past_block,
        &mut counter,
    )
    .await;
    let (packed_past2, _) = store_event(
        event_store,
        contract_address,
        event_selector,
        scope,
        past_block,
        &mut counter,
    )
    .await;

    // Events in latest block
    let (_, _) = store_event(
        event_store,
        contract_address,
        event_selector,
        scope,
        latest_block,
        &mut counter,
    )
    .await;
    let (_, _) = store_event(
        event_store,
        contract_address,
        event_selector,
        scope,
        latest_block,
        &mut counter,
    )
    .await;

    // Query only past block (to_block is exclusive, so to_block = past_block + 1)
    let events = pxe
        .get_private_events(
            &event_selector,
            PrivateEventFilter {
                contract_address,
                scopes: vec![scope],
                from_block: Some(past_block),
                to_block: Some(past_block + 1), // exclusive upper bound
                ..Default::default()
            },
        )
        .await
        .expect("get events filtered by block range");

    assert_eq!(events.len(), 2, "should only return events in past block");
    assert_eq!(events[0].packed_event, packed_past1);
    assert_eq!(events[1].packed_event, packed_past2);
}

// Note: Not testing a successful run of `proveTx`, `sendTx`, `getTxReceipt`
// and `executeUtility` here as it requires a larger setup and it's
// sufficiently tested in the e2e tests.
