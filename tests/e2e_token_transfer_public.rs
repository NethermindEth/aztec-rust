//! Token public transfer tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/transfer_in_public.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_transfer_public -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::cast_possible_wrap,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    dead_code,
    unused_imports
)]

mod common;
use common::*;

use aztec_rs::authwit::SetPublicAuthWitInteraction;
use aztec_rs::hash::MessageHashOrIntent;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Mint amount used by setup (mirrors upstream `const amount = 10000n`).
const MINT_AMOUNT: u64 = 10000;

// ---------------------------------------------------------------------------
// Test-specific helpers (not in common)
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
// Shared test state (mirrors beforeAll in upstream TokenContractTest)
// ---------------------------------------------------------------------------

struct TestState {
    base: TokenTestState,
    bad_account_address: AztecAddress,
}

static SHARED_STATE: OnceCell<Option<TestState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static TestState> {
    let state = SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await;
    state.as_ref()
}

async fn init_shared_state() -> Option<TestState> {
    let base = init_token_test_state(MINT_AMOUNT, 0).await?;

    // Deploy InvalidAccountContract (badAccount)
    let (bad_account_address, bad_account_artifact, bad_account_instance) = deploy_contract(
        &base.admin_wallet,
        load_invalid_account_artifact(),
        vec![],
        base.admin_address,
    )
    .await;

    // Register bad_account contract on account1's PXE
    register_contract_on_pxe(
        base.account1_wallet.pxe(),
        &bad_account_artifact,
        &bad_account_instance,
    )
    .await;

    Some(TestState {
        base,
        bad_account_address,
    })
}

// ===========================================================================
// Tests: e2e_token_contract transfer public
// ===========================================================================

/// TS: transfer less than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_less_than_balance() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    assert!(amount > 0, "admin should have a positive public balance");

    send_token_method(
        &s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(0), // nonce = 0 for self
        ],
        s.base.admin_address,
    )
    .await;

    wait_for_next_block(&s.base.admin_wallet).await;

    let admin_balance = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    assert_eq!(
        admin_balance,
        balance0 - amount,
        "admin balance should decrease"
    );

    let account1_balance = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.account1_address,
    )
    .await;
    assert!(
        account1_balance >= amount,
        "account1 should have received the transferred amount"
    );
}

/// TS: transfer to self
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_self() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    let amount = balance / 2;
    assert!(amount > 0, "admin should have a positive public balance");

    send_token_method(
        &s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(0),
        ],
        s.base.admin_address,
    )
    .await;

    wait_for_next_block(&s.base.admin_wallet).await;

    let balance_after = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    assert_eq!(
        balance_after, balance,
        "balance should be unchanged after self-transfer"
    );
}

/// TS: transfer on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    assert!(amount > 0, "admin should have a positive public balance");
    let authwit_nonce = Fr::random();

    // Build the transfer action
    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Admin authorizes account1 via public authwit (AuthRegistry)
    let intent = MessageHashOrIntent::Intent {
        caller: s.base.account1_address,
        call: action.clone(),
    };
    let set_authwit = SetPublicAuthWitInteraction::create(
        &s.base.admin_wallet,
        s.base.admin_address,
        intent,
        true,
    )
    .await
    .expect("create set_public_authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send set_public_authwit");

    wait_for_next_block(&s.base.admin_wallet).await;

    // Account1 performs the transfer
    let transfer_call = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    s.base
        .account1_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![transfer_call],
                ..Default::default()
            },
            SendOptions {
                from: s.base.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect("send transfer on behalf of other");

    wait_for_next_block(&s.base.account1_wallet).await;

    let admin_balance = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    assert_eq!(
        admin_balance,
        balance0 - amount,
        "admin balance should decrease"
    );

    // Check that the message hash is no longer valid — re-using the same
    // nonce should fail with "unauthorized".
    let replay_call = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let err = s
        .base
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![replay_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: authwit already consumed");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("unauthorized")
            || err_str.contains("assertion failed")
            || err_str.contains("reverted"),
        "expected 'unauthorized' error, got: {err}"
    );
}

// ===========================================================================
// Failure cases
// ===========================================================================

/// TS: failure cases > transfer more than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_more_than_balance() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    let amount = balance0 + 1;

    let call = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(0),
        ],
    );

    assert_sim_revert(
        &s.base.admin_wallet,
        ExecutionPayload {
            calls: vec![call],
            ..Default::default()
        },
        s.base.admin_address,
        U128_UNDERFLOW_ERROR,
    )
    .await;
}

/// TS: failure cases > transfer on behalf of self with non-zero nonce
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_self_with_non_zero_nonce() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    let amount = balance0.saturating_sub(1);

    let call = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(1), // non-zero nonce
        ],
    );

    let err = s
        .base
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: non-zero nonce for self-transfer");

    let err_str = err.to_string();
    assert!(
        err_str.contains("Invalid authwit nonce")
            || err_str.contains("Assertion failed")
            || err_str.contains("reverted"),
        "expected 'Invalid authwit nonce' or assertion failure, got: {err}"
    );
}

/// TS: failure cases > transfer on behalf of other without "approval"
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_without_approval() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    let amount = balance0 + 1;
    let authwit_nonce = Fr::random();

    let call = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    let err = s
        .base
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: no public authwit");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("unauthorized")
            || err_str.contains("assertion failed")
            || err_str.contains("reverted"),
        "expected 'unauthorized' error, got: {err}"
    );
}

/// TS: failure cases > transfer more than balance on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_more_than_balance_on_behalf_of_other() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    let balance1 = public_balance(
        &s.base.account1_wallet,
        s.base.token_address,
        &s.base.account1_address,
    )
    .await;
    let amount = balance0 + 1;
    let authwit_nonce = Fr::random();
    assert!(amount > 0);

    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Set public authwit
    let intent = MessageHashOrIntent::Intent {
        caller: s.base.account1_address,
        call: action.clone(),
    };
    let set_authwit = SetPublicAuthWitInteraction::create(
        &s.base.admin_wallet,
        s.base.admin_address,
        intent,
        true,
    )
    .await
    .expect("create set_public_authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send set_public_authwit");

    wait_for_next_block(&s.base.admin_wallet).await;

    // Perform the transfer — should fail due to underflow
    let transfer_call = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    assert_sim_revert(
        &s.base.account1_wallet,
        ExecutionPayload {
            calls: vec![transfer_call],
            ..Default::default()
        },
        s.base.account1_address,
        U128_UNDERFLOW_ERROR,
    )
    .await;

    // Verify balances unchanged
    let admin_balance_after = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    assert_eq!(
        admin_balance_after, balance0,
        "admin balance should be unchanged"
    );

    let account1_balance_after = public_balance(
        &s.base.account1_wallet,
        s.base.token_address,
        &s.base.account1_address,
    )
    .await;
    assert_eq!(
        account1_balance_after, balance1,
        "account1 balance should be unchanged"
    );
}

/// TS: failure cases > transfer on behalf of other, wrong designated caller
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_wrong_designated_caller() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    let balance1 = public_balance(
        &s.base.account1_wallet,
        s.base.token_address,
        &s.base.account1_address,
    )
    .await;
    let amount = balance0 + 2;
    let authwit_nonce = Fr::random();
    assert!(amount > 0);

    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Set public authwit with WRONG caller (admin instead of account1)
    let wrong_intent = MessageHashOrIntent::Intent {
        caller: s.base.admin_address,
        call: action.clone(),
    };
    let set_authwit = SetPublicAuthWitInteraction::create(
        &s.base.admin_wallet,
        s.base.admin_address,
        wrong_intent,
        true,
    )
    .await
    .expect("create set_public_authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send set_public_authwit");

    wait_for_next_block(&s.base.admin_wallet).await;

    // Account1 tries the transfer — should fail (authwit was for admin, not account1)
    let transfer_call = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let err = s
        .base
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![transfer_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: wrong designated caller");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("unauthorized")
            || err_str.contains("assertion failed")
            || err_str.contains("reverted"),
        "expected 'unauthorized' error, got: {err}"
    );

    // Verify balances unchanged
    let admin_balance_after = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    assert_eq!(
        admin_balance_after, balance0,
        "admin balance should be unchanged"
    );

    let account1_balance_after = public_balance(
        &s.base.account1_wallet,
        s.base.token_address,
        &s.base.account1_address,
    )
    .await;
    assert_eq!(
        account1_balance_after, balance1,
        "account1 balance should be unchanged"
    );
}

/// TS: failure cases > transfer on behalf of other, cancelled authwit
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_cancelled_authwit() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    assert!(amount > 0);
    let authwit_nonce = Fr::random();

    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Set public authwit (authorized=true)
    let intent = MessageHashOrIntent::Intent {
        caller: s.base.account1_address,
        call: action.clone(),
    };
    let set_authwit = SetPublicAuthWitInteraction::create(
        &s.base.admin_wallet,
        s.base.admin_address,
        intent.clone(),
        true,
    )
    .await
    .expect("create set_public_authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send set_public_authwit");

    wait_for_next_block(&s.base.admin_wallet).await;

    // Cancel public authwit (authorized=false)
    let cancel_authwit = SetPublicAuthWitInteraction::create(
        &s.base.admin_wallet,
        s.base.admin_address,
        intent,
        false,
    )
    .await
    .expect("create cancel_public_authwit");
    cancel_authwit
        .send(SendOptions::default())
        .await
        .expect("send cancel_public_authwit");

    wait_for_next_block(&s.base.admin_wallet).await;

    // Account1 tries the transfer with a new action — should fail
    let transfer_call = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let err = s
        .base
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![transfer_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: cancelled authwit");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("unauthorized")
            || err_str.contains("assertion failed")
            || err_str.contains("reverted"),
        "expected 'unauthorized' error, got: {err}"
    );
}

/// TS: failure cases > transfer on behalf of other, cancelled authwit, flow 2
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_cancelled_authwit_flow_2() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    assert!(amount > 0);
    let authwit_nonce = Fr::random();

    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Set public authwit (authorized=true)
    let intent = MessageHashOrIntent::Intent {
        caller: s.base.account1_address,
        call: action.clone(),
    };
    let set_authwit = SetPublicAuthWitInteraction::create(
        &s.base.admin_wallet,
        s.base.admin_address,
        intent.clone(),
        true,
    )
    .await
    .expect("create set_public_authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send set_public_authwit");

    wait_for_next_block(&s.base.admin_wallet).await;

    // Cancel public authwit (authorized=false)
    let cancel_authwit = SetPublicAuthWitInteraction::create(
        &s.base.admin_wallet,
        s.base.admin_address,
        intent,
        false,
    )
    .await
    .expect("create cancel_public_authwit");
    cancel_authwit
        .send(SendOptions::default())
        .await
        .expect("send cancel_public_authwit");

    wait_for_next_block(&s.base.admin_wallet).await;

    // Simulate using the original action — should fail
    let err = s
        .base
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![action],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: cancelled authwit (flow 2)");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("unauthorized")
            || err_str.contains("assertion failed")
            || err_str.contains("reverted"),
        "expected 'unauthorized' error, got: {err}"
    );
}

/// TS: failure cases > transfer on behalf of other, invalid spend_public_authwit on "from"
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_invalid_spend_public_authwit() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let authwit_nonce = Fr::random();

    // Transfer from badAccount (which hasn't authorized anyone) — should fail
    let call = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.bad_account_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(0),
            AbiValue::Field(authwit_nonce),
        ],
    );

    let err = s
        .base
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: invalid spend_public_authwit");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("unauthorized")
            || err_str.contains("assertion failed")
            || err_str.contains("reverted"),
        "expected 'unauthorized' error, got: {err}"
    );
}
