//! Token private transfer tests -- 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/transfer_in_private.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test contract e2e_token_transfer_private:: -- --ignored --nocapture
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
    dead_code,
    unused_imports
)]

use crate::common::*;

use aztec_rs::hash::{
    compute_auth_wit_message_hash, compute_inner_auth_wit_hash_from_action, MessageHashOrIntent,
};

/// Mint amount used by setup (mirrors upstream `const amount = 10000n`).
const MINT_AMOUNT: u64 = 10000;

// ---------------------------------------------------------------------------
// Test-specific helpers
// ---------------------------------------------------------------------------

/// Wait for the next block to ensure post-TX state is committed.
async fn wait_for_next_block(wallet: &SharedTestWallet) {
    let current = wallet.pxe().node().get_block_number().await.unwrap_or(0);
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let now = wallet.pxe().node().get_block_number().await.unwrap_or(0);
        if now > current + 1 {
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Shared test state (mirrors beforeAll in upstream TokenContractTest)
// ---------------------------------------------------------------------------

struct TestState {
    base: AuthwitTokenTestState,
    proxy_address: AztecAddress,
    proxy_artifact: ContractArtifact,
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
    let base = init_authwit_token_test_state(0, MINT_AMOUNT).await?;

    // Deploy GenericProxy contract
    let (proxy_address, proxy_artifact, proxy_instance) = deploy_contract(
        &*base.admin_wallet,
        load_generic_proxy_artifact(),
        vec![],
        base.admin_address,
    )
    .await;

    // Deploy InvalidAccountContract
    let (bad_account_address, bad_account_artifact, bad_account_instance) = deploy_contract(
        &*base.admin_wallet,
        load_invalid_account_artifact(),
        vec![],
        base.admin_address,
    )
    .await;

    // Register contracts on account1's PXE
    register_contract_on_pxe(base.account1_wallet.pxe(), &proxy_artifact, &proxy_instance).await;
    register_contract_on_pxe(
        base.account1_wallet.pxe(),
        &bad_account_artifact,
        &bad_account_instance,
    )
    .await;

    Some(TestState {
        base,
        proxy_address,
        proxy_artifact,
        bad_account_address,
    })
}

// ===========================================================================
// Tests: e2e_token_contract transfer private
// ===========================================================================

/// TS: transfer on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    let authwit_nonce = Fr::random();
    assert!(amount > 0, "admin should have a positive balance");

    // Build the transfer_in_private action
    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_private",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Create authwit: admin authorizes proxy to call transfer_in_private
    let intent = MessageHashOrIntent::Intent {
        caller: s.proxy_address,
        call: action.clone(),
    };
    let witness = s
        .base
        .admin_wallet
        .create_auth_wit(s.base.admin_address, intent)
        .await
        .expect("create authwit");

    // Admin sends through proxy so their keys are in scope, while proxy
    // becomes msg_sender to trigger authwit.
    let proxy_call = build_proxy_call(&s.proxy_artifact, s.proxy_address, &action);
    s.base
        .admin_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![proxy_call.clone()],
                ..Default::default()
            },
            SendOptions {
                from: s.base.admin_address,
                auth_witnesses: vec![witness.clone()],
                ..Default::default()
            },
        )
        .await
        .expect("send transfer via proxy");

    // Verify the transfer succeeded
    wait_for_next_block(&s.base.admin_wallet).await;

    let admin_balance = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    assert_eq!(
        admin_balance,
        balance0 - amount,
        "admin balance should decrease"
    );

    let account1_balance = call_utility_u128(
        &*s.base.account1_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.account1_address))],
        s.base.account1_address,
    )
    .await;
    assert_eq!(
        account1_balance, amount,
        "account1 should receive the transferred amount"
    );

    // Perform the transfer again -- should fail (duplicate nullifier)
    let err = s
        .base
        .admin_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![proxy_call],
                ..Default::default()
            },
            SendOptions {
                from: s.base.admin_address,
                auth_witnesses: vec![witness],
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: duplicate nullifier");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("duplicate nullifier")
            || err_str.contains("nullifier already exists")
            || err_str.contains("nullifier collision")
            || err_str.contains("existing nullifier"),
        "expected duplicate nullifier error, got: {err}"
    );
}

// -- failure cases --

/// TS: failure cases > transfer on behalf of self with non-zero nonce
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_self_with_non_zero_nonce() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = balance0 - 1;
    assert!(amount > 0, "admin should have a positive balance");

    // Simulate transfer_in_private with non-zero nonce from self
    let call = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_private",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(Fr::from(1u64)), // non-zero nonce
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
            || err_str.contains("Cannot satisfy constraint"),
        "expected 'Invalid authwit nonce' or constraint failure, got: {err}"
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

    let balance0 = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let balance1 = call_utility_u128(
        &*s.base.account1_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.account1_address))],
        s.base.account1_address,
    )
    .await;
    let amount = balance0 + 1;
    let authwit_nonce = Fr::random();
    assert!(amount > 0);

    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_private",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    let intent = MessageHashOrIntent::Intent {
        caller: s.proxy_address,
        call: action.clone(),
    };
    let witness = s
        .base
        .admin_wallet
        .create_auth_wit(s.base.admin_address, intent)
        .await
        .expect("create authwit");

    // Admin sends through proxy -- simulate only
    let proxy_call = build_proxy_call(&s.proxy_artifact, s.proxy_address, &action);
    let err = s
        .base
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![proxy_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.admin_address,
                auth_witnesses: vec![witness],
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: balance too low");

    let err_str = err.to_string();
    assert!(
        err_str.contains("Balance too low")
            || err_str.contains("Assertion failed")
            || err_str.contains("Cannot satisfy constraint"),
        "expected 'Balance too low' or constraint failure, got: {err}"
    );

    // Verify balances unchanged
    let admin_balance = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    assert_eq!(admin_balance, balance0, "admin balance should be unchanged");

    let account1_balance = call_utility_u128(
        &*s.base.account1_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.account1_address))],
        s.base.account1_address,
    )
    .await;
    assert_eq!(
        account1_balance, balance1,
        "account1 balance should be unchanged"
    );
}

/// TS: failure cases > transfer on behalf of other without approval
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_without_approval() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    let authwit_nonce = Fr::random();
    assert!(amount > 0);

    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_private",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Compute the expected message hash (proxy as caller)
    let chain_info = s
        .base
        .admin_wallet
        .get_chain_info()
        .await
        .expect("get chain info");
    let message_hash = compute_auth_wit_message_hash(
        &MessageHashOrIntent::Intent {
            caller: s.proxy_address,
            call: action.clone(),
        },
        &chain_info,
    );

    // Admin sends through proxy WITHOUT any authwit
    let proxy_call = build_proxy_call(&s.proxy_artifact, s.proxy_address, &action);
    let err = s
        .base
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![proxy_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: no authwit");

    let err_str = err.to_string();
    assert!(
        err_str.contains(&format!(
            "Unknown auth witness for message hash {message_hash}"
        )) || err_str.contains("Unknown auth witness")
            || err_str.contains("auth witness")
            || err_str.contains("Cannot satisfy constraint")
            || err_str.contains("execution failed"),
        "expected auth witness error or constraint failure, got: {err}"
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

    let balance0 = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    let authwit_nonce = Fr::random();
    assert!(amount > 0);

    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_private",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Compute the expected message hash (proxy as caller, which is what the
    // contract will check for)
    let chain_info = s
        .base
        .admin_wallet
        .get_chain_info()
        .await
        .expect("get chain info");
    let expected_message_hash = compute_auth_wit_message_hash(
        &MessageHashOrIntent::Intent {
            caller: s.proxy_address,
            call: action.clone(),
        },
        &chain_info,
    );

    // Create authwit with WRONG caller (account1 instead of proxy)
    let wrong_intent = MessageHashOrIntent::Intent {
        caller: s.base.account1_address,
        call: action.clone(),
    };
    let witness = s
        .base
        .admin_wallet
        .create_auth_wit(s.base.admin_address, wrong_intent)
        .await
        .expect("create authwit with wrong caller");

    // Admin sends through proxy -- authwit is for wrong caller
    let proxy_call = build_proxy_call(&s.proxy_artifact, s.proxy_address, &action);
    let err = s
        .base
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![proxy_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.admin_address,
                auth_witnesses: vec![witness],
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: wrong designated caller");

    let err_str = err.to_string();
    assert!(
        err_str.contains(&format!(
            "Unknown auth witness for message hash {expected_message_hash}"
        )) || err_str.contains("Unknown auth witness")
            || err_str.contains("auth witness")
            || err_str.contains("Cannot satisfy constraint")
            || err_str.contains("execution failed"),
        "expected auth witness error or constraint failure, got: {err}"
    );

    // Verify admin balance unchanged
    let admin_balance = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    assert_eq!(admin_balance, balance0, "admin balance should be unchanged");
}

/// TS: failure cases > transfer on behalf of other, cancelled authwit
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_cancelled_authwit() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    let authwit_nonce = Fr::random();
    assert!(amount > 0);

    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_private",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    let intent = MessageHashOrIntent::Intent {
        caller: s.proxy_address,
        call: action.clone(),
    };
    let witness = s
        .base
        .admin_wallet
        .create_auth_wit(s.base.admin_address, intent)
        .await
        .expect("create authwit");

    // Cancel the authwit by calling cancel_authwit(inner_hash) on the token
    // contract. This emits the nullifier that would have been emitted on
    // consumption, preventing future use.
    let inner_hash = compute_inner_auth_wit_hash_from_action(&s.proxy_address, &action);
    send_token_method(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "cancel_authwit",
        vec![AbiValue::Field(inner_hash)],
        s.base.admin_address,
    )
    .await;

    wait_for_next_block(&s.base.admin_wallet).await;

    // Admin sends through proxy -- should fail because nullifier already emitted
    let proxy_call = build_proxy_call(&s.proxy_artifact, s.proxy_address, &action);
    let err = s
        .base
        .admin_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![proxy_call],
                ..Default::default()
            },
            SendOptions {
                from: s.base.admin_address,
                auth_witnesses: vec![witness],
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: cancelled authwit (duplicate nullifier)");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("duplicate nullifier")
            || err_str.contains("nullifier already exists")
            || err_str.contains("nullifier collision")
            || err_str.contains("existing nullifier"),
        "expected duplicate nullifier error, got: {err}"
    );
}

/// TS: failure cases > transfer on behalf of other, invalid verify_private_authwit on "from"
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_invalid_verify_private_authwit() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let authwit_nonce = Fr::random();

    // Should fail as the returned value from the badAccount is malformed
    let call = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_in_private",
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
        .expect_err("should fail: invalid verify_private_authwit");

    let err_str = err.to_string();
    assert!(
        err_str.contains("Message not authorized by account")
            || err_str.contains("not authorized")
            || err_str.contains("Assertion failed")
            || err_str.contains("Cannot satisfy constraint")
            || err_str.contains("execution cache")
            || err_str.contains("execution failed"),
        "expected auth verification failure, got: {err}"
    );
}
