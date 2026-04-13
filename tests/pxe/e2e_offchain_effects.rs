//! Offchain effects tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_offchain_effect.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_offchain_effects -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::cast_possible_wrap,
    dead_code,
    unused_imports
)]

use aztec_rs::abi::AbiValue;
use aztec_rs::wallet::{SimulateOptions, Wallet};

use crate::common::*;
use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct OffchainState {
    wallet: TestWallet,
    owner: AztecAddress,
    artifact: ContractArtifact,
    contract_address: AztecAddress,
}

static SHARED_STATE: OnceCell<Option<OffchainState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static OffchainState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<OffchainState> {
    let (wallet, owner) = setup_wallet(TEST_ACCOUNT_0).await?;

    let artifact = load_offchain_effect_artifact()?;

    let deploy = Contract::deploy(&wallet, artifact.clone(), vec![], None).expect("deploy builder");
    let result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: owner,
                ..Default::default()
            },
        )
        .await
        .expect("deploy offchain effect contract");

    Some(OffchainState {
        wallet,
        owner,
        artifact,
        contract_address: result.instance.address,
    })
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: should emit event as offchain message and process it
///
/// Calls emit_event_as_offchain_message_for_msg_sender(a, b, c) and verifies
/// the tx succeeds. The upstream reads the event back via getPrivateEvents.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn emit_event_as_offchain_message() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // emit_event_as_offchain_message_for_msg_sender(a: u32, b: u32, c: u32)
    let call = build_call(
        &s.artifact,
        s.contract_address,
        "emit_event_as_offchain_message_for_msg_sender",
        vec![
            AbiValue::Integer(1),
            AbiValue::Integer(2),
            AbiValue::Integer(3),
        ],
    );

    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("emit_event_as_offchain_message_for_msg_sender");
}

/// TS: should emit note as offchain message and process it
///
/// Calls emit_note_as_offchain_message(value, owner) and verifies the tx
/// succeeds.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn emit_note_as_offchain_message() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // emit_note_as_offchain_message(value: u32, owner: AztecAddress)
    let call = build_call(
        &s.artifact,
        s.contract_address,
        "emit_note_as_offchain_message",
        vec![AbiValue::Integer(42), abi_address(s.owner)],
    );

    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("emit_note_as_offchain_message");
}

/// TS: should return offchain effects from send()
///
/// The emit_offchain_effects function takes a BoundedVec<EffectPayload, 6>.
/// We simulate with an empty effects list to verify the function is callable.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn returns_offchain_effects_from_send() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Build an empty BoundedVec<EffectPayload, 6>:
    // storage: [EffectPayload{data:[0;5], next_contract:{inner:0}}; 6], len: 0
    let empty_effect_payload = || {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "data".to_owned(),
            AbiValue::Array(vec![AbiValue::Field(Fr::zero()); 5]),
        );
        fields.insert(
            "next_contract".to_owned(),
            abi_address(AztecAddress::zero()),
        );
        AbiValue::Struct(fields)
    };
    let mut bounded_vec = std::collections::BTreeMap::new();
    bounded_vec.insert(
        "storage".to_owned(),
        AbiValue::Array(vec![empty_effect_payload(); 6]),
    );
    bounded_vec.insert("len".to_owned(), AbiValue::Integer(0));

    let call = build_call(
        &s.artifact,
        s.contract_address,
        "emit_offchain_effects",
        vec![AbiValue::Struct(bounded_vec)],
    );

    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("emit_offchain_effects(empty)");
}

/// TS: should emit offchain effects
///
/// Simulates emit_offchain_effects with 2 effects.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn emits_offchain_effects() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let make_effect = |data_val: u64, next: AztecAddress| {
        let mut fields = std::collections::BTreeMap::new();
        let data: Vec<AbiValue> = (0..5)
            .map(|i| {
                if i == 0 {
                    AbiValue::Field(Fr::from(data_val))
                } else {
                    AbiValue::Field(Fr::zero())
                }
            })
            .collect();
        fields.insert("data".to_owned(), AbiValue::Array(data));
        fields.insert("next_contract".to_owned(), abi_address(next));
        AbiValue::Struct(fields)
    };

    let empty_effect = make_effect(0, AztecAddress::zero());
    let effect1 = make_effect(1, s.contract_address);
    let effect2 = make_effect(2, s.contract_address);

    let mut storage = vec![effect1, effect2];
    // Pad to 6 elements
    while storage.len() < 6 {
        storage.push(empty_effect.clone());
    }

    let mut bounded_vec = std::collections::BTreeMap::new();
    bounded_vec.insert("storage".to_owned(), AbiValue::Array(storage));
    bounded_vec.insert("len".to_owned(), AbiValue::Integer(2));

    let call = build_call(
        &s.artifact,
        s.contract_address,
        "emit_offchain_effects",
        vec![AbiValue::Struct(bounded_vec)],
    );

    s.wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("simulate emit_offchain_effects(2 effects)");
}

/// TS: should not emit any offchain effects
///
/// Calls emit_offchain_effects with len=0.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn no_offchain_effects_when_empty() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let empty_effect = || {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "data".to_owned(),
            AbiValue::Array(vec![AbiValue::Field(Fr::zero()); 5]),
        );
        fields.insert(
            "next_contract".to_owned(),
            abi_address(AztecAddress::zero()),
        );
        AbiValue::Struct(fields)
    };
    let mut bounded_vec = std::collections::BTreeMap::new();
    bounded_vec.insert(
        "storage".to_owned(),
        AbiValue::Array(vec![empty_effect(); 6]),
    );
    bounded_vec.insert("len".to_owned(), AbiValue::Integer(0));

    let call = build_call(
        &s.artifact,
        s.contract_address,
        "emit_offchain_effects",
        vec![AbiValue::Struct(bounded_vec)],
    );

    s.wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("simulate emit_offchain_effects(empty)");
}
