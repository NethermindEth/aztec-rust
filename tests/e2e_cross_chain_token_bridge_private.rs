//! Token bridge private tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_cross_chain_messaging/token_bridge_private.test.ts`.
//!
//! These tests require an L1 bridge harness + TokenPortal + TokenBridge contracts.
//! Until the Ethereum integration layer is implemented, tests skip gracefully.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_cross_chain_token_bridge_private -- --ignored --nocapture
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

struct TokenBridgeState {
    wallet: TestWallet,
    owner: AztecAddress,
}

static SHARED_STATE: OnceCell<Option<TokenBridgeState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static TokenBridgeState> {
    SHARED_STATE
        .get_or_init(|| async {
            let (wallet, owner) = setup_wallet(TEST_ACCOUNT_0).await?;
            Some(TokenBridgeState { wallet, owner })
        })
        .await
        .as_ref()
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: Privately deposit funds from L1 -> L2 and withdraw back to L1
///
/// Requires L1 TokenPortal, TokenBridge contracts, and bridge harness.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_deposit_l1_to_l2_and_withdraw() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    // TODO: Implement when L1 bridge harness is available:
    // 1. Mint tokens on L1
    // 2. Deposit via TokenPortal.depositToAztecPublic
    // 3. Consume L1→L2 message on L2
    // 4. Mint private tokens via TokenBridge.claim_private
    // 5. Authorize withdrawal via authwit
    // 6. Withdraw back to L1 via TokenBridge.exit_to_l1_private
    // 7. Consume L2→L1 message on L1 via Outbox
    // 8. Verify final L1 + L2 balances
    eprintln!("skipping: L1 bridge harness not yet available");
}

/// TS: Claim secret is enough to consume the message
///
/// Requires L1 bridge harness.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn claim_secret_consumes_message() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    // TODO: Implement when L1 bridge harness is available:
    // 1. Deposit from L1 via TokenPortal
    // 2. Use a different account to claim via TokenBridge.claim_private
    //    (only claim_secret needed, not the depositor)
    // 3. Verify the claiming account receives the private tokens
    eprintln!("skipping: L1 bridge harness not yet available");
}
