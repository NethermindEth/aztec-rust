//! L1 to L2 messaging tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_cross_chain_messaging/l1_to_l2.test.ts`.
//!
//! These tests require an L1 bridge test harness to send messages from L1 to L2.
//! Until the Ethereum integration layer is implemented, tests skip gracefully.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_cross_chain_l1_to_l2 -- --ignored --nocapture
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
// Shared state
// ---------------------------------------------------------------------------

struct L1ToL2State {
    wallet: TestWallet,
    owner: AztecAddress,
}

static SHARED_STATE: OnceCell<Option<L1ToL2State>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static L1ToL2State> {
    SHARED_STATE
        .get_or_init(|| async {
            let (wallet, owner) = setup_wallet(TEST_ACCOUNT_0).await?;
            Some(L1ToL2State { wallet, owner })
        })
        .await
        .as_ref()
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: can send an L1 to L2 message from a non-registered portal address
///     consumed from private/public repeatedly
///
/// Requires L1 bridge harness (Ethereum integration).
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn l1_to_l2_message_consumed_private_public() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    // TODO: Implement when L1 bridge harness is available:
    // 1. Send L1→L2 message via Inbox contract on L1
    // 2. Wait for message to be included in L2 block
    // 3. Consume via TestContract.consume_message_from_arbitrary_sender_private
    // 4. Consume via TestContract.consume_message_from_arbitrary_sender_public
    // 5. Verify duplicate consumption works (same content, different leaf)
    eprintln!("skipping: L1 bridge harness not yet available");
}

/// TS: can consume L1 to L2 message in private/public after inbox drifts
///     away from the rollup
///
/// Requires L1 bridge harness.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn l1_to_l2_message_after_inbox_drift() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    // TODO: Implement when L1 bridge harness is available:
    // 1. Send L1→L2 message
    // 2. Advance L1 blocks so inbox drifts from rollup
    // 3. Wait for correct checkpoint alignment
    // 4. Consume message privately and publicly
    eprintln!("skipping: L1 bridge harness not yet available");
}
