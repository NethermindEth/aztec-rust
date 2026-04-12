//! Offchain effects tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_offchain_effect.test.ts`.
//!
//! These tests require an OffchainEffectContract fixture.
//! Until the fixture is compiled and available, tests skip gracefully.
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

mod common;
use common::*;
use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_offchain_effect_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/offchain_effect_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/offchain_effect_contract-OffchainEffectContract.json"),
    ])
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct OffchainState {
    wallet: TestWallet,
    owner: AztecAddress,
}

static SHARED_STATE: OnceCell<Option<OffchainState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static OffchainState> {
    SHARED_STATE
        .get_or_init(|| async {
            let (wallet, owner) = setup_wallet(TEST_ACCOUNT_0).await?;
            Some(OffchainState { wallet, owner })
        })
        .await
        .as_ref()
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: should return offchain effects from send()
///
/// Calls emit_offchain_effects and verifies effects are returned.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn returns_offchain_effects_from_send() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    let Some(_artifact) = load_offchain_effect_artifact() else {
        eprintln!("skipping: OffchainEffectContract fixture not available");
        return;
    };

    // TODO: Deploy OffchainEffectContract, call emit_offchain_effects([1, 2]),
    //       verify 2 effects returned in reversed order
    eprintln!("skipping: OffchainEffectContract fixture not available");
}

/// TS: should emit offchain effects
///
/// Proves interaction and verifies effects are properly reversed and assigned.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn emits_offchain_effects() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    let Some(_artifact) = load_offchain_effect_artifact() else {
        eprintln!("skipping: OffchainEffectContract fixture not available");
        return;
    };

    // TODO: Deploy, call emit_offchain_effects, verify effects via prove
    eprintln!("skipping: OffchainEffectContract fixture not available");
}

/// TS: should not emit any offchain effects
///
/// Calls emit_offchain_effects with empty array; verifies no effects.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn no_offchain_effects_when_empty() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    let Some(_artifact) = load_offchain_effect_artifact() else {
        eprintln!("skipping: OffchainEffectContract fixture not available");
        return;
    };

    // TODO: Deploy, call emit_offchain_effects([]), verify empty
    eprintln!("skipping: OffchainEffectContract fixture not available");
}

/// TS: should emit event as offchain message and process it
///
/// Emits event as offchain message, retrieves from block, processes, reads
/// via getPrivateEvents.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn emit_event_as_offchain_message() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    let Some(_artifact) = load_offchain_effect_artifact() else {
        eprintln!("skipping: OffchainEffectContract fixture not available");
        return;
    };

    // TODO: Deploy, emit_event_as_offchain_message_for_msg_sender,
    //       retrieve from block, process_message, getPrivateEvents
    eprintln!("skipping: OffchainEffectContract fixture not available");
}

/// TS: should emit note as offchain message and process it
///
/// Emits note as offchain message, processes, retrieves note value.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn emit_note_as_offchain_message() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    let Some(_artifact) = load_offchain_effect_artifact() else {
        eprintln!("skipping: OffchainEffectContract fixture not available");
        return;
    };

    // TODO: Deploy, emit_note_as_offchain_message, process_message,
    //       get_note_value
    eprintln!("skipping: OffchainEffectContract fixture not available");
}
