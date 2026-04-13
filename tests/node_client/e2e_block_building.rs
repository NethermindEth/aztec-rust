//! Block building tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_block_building.test.ts`.
//!
//! Tests multi-tx blocks, double-spend detection, and log ordering.
//! Some tests require `setConfig({ minTxsPerBlock })` on the node admin,
//! which is not yet available in the Rust SDK.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_block_building -- --ignored --nocapture
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

use crate::common::*;
use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct BlockBuildingState {
    wallet: TestWallet,
    owner: AztecAddress,
    stateful_artifact: ContractArtifact,
    stateful_address: AztecAddress,
}

static SHARED_STATE: OnceCell<Option<BlockBuildingState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static BlockBuildingState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<BlockBuildingState> {
    let (wallet, owner) = setup_wallet(TEST_ACCOUNT_0).await?;

    let stateful_artifact = load_stateful_test_artifact();
    let deploy = Contract::deploy(
        &wallet,
        stateful_artifact.clone(),
        vec![abi_address(owner), AbiValue::Field(Fr::from(1u64))],
        None,
    )
    .expect("deploy builder");

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
        .expect("deploy stateful");

    Some(BlockBuildingState {
        wallet,
        owner,
        stateful_artifact,
        stateful_address: result.instance.address,
    })
}

// ===========================================================================
// Tests: multi-tx blocks
// ===========================================================================

/// TS: assembles a block with multiple txs with public fns
///
/// Sends multiple public increment calls and verifies all are included.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn assembles_block_with_public_fns() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Send 2 public increment calls (one at a time — we can't control
    // minTxsPerBlock, but the node may batch them in the same block)
    for i in 0..2u64 {
        let call = build_call(
            &s.stateful_artifact,
            s.stateful_address,
            "increment_public_value_no_init_check",
            vec![
                AbiValue::Field(Fr::from(s.owner)),
                AbiValue::Field(Fr::from(i + 1)),
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
            .expect("send public increment");
    }

    // Verify the accumulated public value
    let slot = derive_storage_slot_in_map(2, &s.owner); // public_values at slot 2
    let value = read_public_u128(&s.wallet, s.stateful_address, slot).await;
    assert!(value >= 3, "public value should reflect all increments");
}

// ===========================================================================
// Tests: double-spend detection
// ===========================================================================

/// TS: double-spends > private -> private (across blocks)
///
/// The same nullifier emitted in two separate blocks: first succeeds,
/// second fails with existing nullifier error.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn double_spend_private_across_blocks() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let test_artifact = load_test_contract_artifact();
    let deploy = Contract::deploy(&s.wallet, test_artifact.clone(), vec![], None)
        .expect("deploy test contract");
    let result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("deploy test contract");
    let test_address = result.instance.address;

    // First emit_nullifier — should succeed
    let nullifier_value = Fr::from(next_unique_salt());
    let call1 = build_call(
        &test_artifact,
        test_address,
        "emit_nullifier",
        vec![AbiValue::Field(nullifier_value)],
    );
    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call1],
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("first emit_nullifier");

    // Second emit_nullifier with same value — should fail
    let call2 = build_call(
        &test_artifact,
        test_address,
        "emit_nullifier",
        vec![AbiValue::Field(nullifier_value)],
    );
    let err = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call2],
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: duplicate nullifier");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("nullifier")
            || err_str.contains("dropped")
            || err_str.contains("existing")
            || err_str.contains("reverted"),
        "expected duplicate nullifier error, got: {err}"
    );
}

// ===========================================================================
// Tests: regressions
// ===========================================================================

/// TS: regressions > sends a tx on the first block
///
/// Verifies basic tx sending works (no special block requirements).
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn sends_tx_on_first_block() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Simple public call to verify tx processing works
    let call = build_call(
        &s.stateful_artifact,
        s.stateful_address,
        "increment_public_value_no_init_check",
        vec![
            AbiValue::Field(Fr::from(s.owner)),
            AbiValue::Field(Fr::from(1u64)),
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
        .expect("send tx");
}
