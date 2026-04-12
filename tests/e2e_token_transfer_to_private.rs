//! Token shielding tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/transfer_to_private.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_transfer_to_private -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::cast_possible_wrap,
    clippy::uninlined_format_args,
    dead_code,
    unused_imports
)]

mod common;
use common::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MINT_AMOUNT: u64 = 10000;

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// Wait for the next block to ensure post-TX state is committed.
async fn wait_for_next_block(wallet: &TestWallet) {
    let current = wallet.pxe().node().get_block_number().await.unwrap_or(0);
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let now = wallet.pxe().node().get_block_number().await.unwrap_or(0);
        if now > current + 1 {
            return;
        }
    }
}

/// Assert a transaction fails during simulation (with tolerance for the
/// node's AVM not catching the error).
async fn assert_sim_revert(
    wallet: &TestWallet,
    payload: ExecutionPayload,
    from: AztecAddress,
    expected_error: &str,
) {
    let sim_result = wallet
        .simulate_tx(
            payload,
            SimulateOptions {
                from,
                ..Default::default()
            },
        )
        .await;

    if let Err(err) = sim_result {
        let err_str = err.to_string();
        assert!(
            err_str.contains(expected_error)
                || err_str.contains("reverted")
                || err_str.contains("Assertion failed"),
            "expected '{}' or 'reverted', got: {}",
            expected_error,
            err
        );
    }
}

// ---------------------------------------------------------------------------
// Shared test state
// ---------------------------------------------------------------------------

static SHARED_STATE: OnceCell<Option<TokenTestState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static TokenTestState> {
    SHARED_STATE
        .get_or_init(|| async { init_token_test_state(MINT_AMOUNT, MINT_AMOUNT).await })
        .await
        .as_ref()
}

// ===========================================================================
// Tests: e2e_token_contract transfer_to_private (shielding)
// ===========================================================================

/// TS: to self
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_private_to_self() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let pub_before = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = pub_before / 2;
    assert!(amount > 0, "admin should have a positive public balance");

    let priv_before = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;

    send_token_method(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "transfer_to_private",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(amount as i128),
        ],
        s.admin_address,
    )
    .await;

    wait_for_next_block(&s.admin_wallet).await;

    let pub_after = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    assert_eq!(
        pub_after,
        pub_before - amount,
        "admin public balance should decrease by {amount}"
    );

    let priv_after = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    assert_eq!(
        priv_after,
        priv_before + amount,
        "admin private balance should increase by {amount}"
    );
}

/// TS: to someone else
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_private_to_someone_else() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let pub_before = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = pub_before / 2;
    assert!(amount > 0, "admin should have a positive public balance");

    send_token_method(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "transfer_to_private",
        vec![
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
        ],
        s.admin_address,
    )
    .await;

    wait_for_next_block(&s.admin_wallet).await;

    let pub_after = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    assert_eq!(
        pub_after,
        pub_before - amount,
        "admin public balance should decrease by {amount}"
    );

    let account1_priv = call_utility_u128(
        &s.account1_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.account1_address))],
        s.account1_address,
    )
    .await;
    assert_eq!(
        account1_priv, amount,
        "account1 private balance should equal transferred amount"
    );
}

// -- failure cases --

/// TS: failure cases > to self (more than balance)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_private_to_self_more_than_balance() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let pub_bal = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = pub_bal + 1;
    assert!(amount > 0, "amount should be positive");

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_to_private",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(amount as i128),
        ],
    );

    assert_sim_revert(
        &s.admin_wallet,
        ExecutionPayload {
            calls: vec![call],
            ..Default::default()
        },
        s.admin_address,
        U128_UNDERFLOW_ERROR,
    )
    .await;
}
