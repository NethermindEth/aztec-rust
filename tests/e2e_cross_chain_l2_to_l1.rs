//! L2 to L1 messaging tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_cross_chain_messaging/l2_to_l1.test.ts`.
//!
//! These tests require L1 integration to consume messages on L1.
//! Until the Ethereum integration layer is implemented, tests skip gracefully.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_cross_chain_l2_to_l1 -- --ignored --nocapture
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

struct L2ToL1State {
    wallet: TestWallet,
    owner: AztecAddress,
}

static SHARED_STATE: OnceCell<Option<L2ToL1State>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static L2ToL1State> {
    SHARED_STATE
        .get_or_init(|| async {
            let (wallet, owner) = setup_wallet(TEST_ACCOUNT_0).await?;
            Some(L2ToL1State { wallet, owner })
        })
        .await
        .as_ref()
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: 1 tx with 2 messages, one from public, one from private, to a
///     non-registered portal address
///
/// Requires L1 Outbox contract to consume messages.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn two_messages_private_and_public() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    // TODO: Implement when L1 integration is available:
    // 1. Batch: create_l2_to_l1_message_arbitrary_recipient_private +
    //           create_l2_to_l1_message_arbitrary_recipient_public
    // 2. Wait for block inclusion
    // 3. Consume both messages on L1 via Outbox contract
    eprintln!("skipping: L1 integration not yet available");
}

/// TS: 2 txs in the same block, one with no messages, one with a message
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn two_txs_one_empty_one_with_message() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    // TODO: Requires L1 Outbox + block building control (minTxsPerBlock)
    eprintln!("skipping: L1 integration not yet available");
}

/// TS: 2 txs (balanced), one with 3 messages (unbalanced), one with 4
///     messages (balanced)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn balanced_and_unbalanced_message_trees() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    // TODO: Requires L1 Outbox + message tree validation
    eprintln!("skipping: L1 integration not yet available");
}

/// TS: 3 txs (unbalanced), one with 3 messages, one with 1 message (subtree
///     root), one with 2 messages (balanced)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn complex_unbalanced_message_tree() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    // TODO: Requires L1 Outbox + complex tree scenarios
    eprintln!("skipping: L1 integration not yet available");
}
