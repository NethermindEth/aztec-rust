//! Token unshielding tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/transfer_to_public.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_transfer_to_public -- --ignored --nocapture
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

use aztec_rs::hash::{compute_auth_wit_message_hash, MessageHashOrIntent};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Mint amount used by setup (mirrors upstream `const amount = 10000n`).
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

// ---------------------------------------------------------------------------
// Shared test state (mirrors beforeAll in upstream TokenContractTest)
// ---------------------------------------------------------------------------

struct TestState {
    base: AuthwitTokenTestState,
    proxy_address: AztecAddress,
    proxy_artifact: ContractArtifact,
}

static SHARED_STATE: OnceCell<Option<TestState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static TestState> {
    let state = SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await;
    state.as_ref()
}

async fn init_shared_state() -> Option<TestState> {
    let base = init_authwit_token_test_state(MINT_AMOUNT, MINT_AMOUNT).await?;

    // Deploy GenericProxy contract on admin's wallet
    let (proxy_address, proxy_artifact, proxy_instance) = deploy_contract(
        &*base.admin_wallet,
        load_generic_proxy_artifact(),
        vec![],
        base.admin_address,
    )
    .await;

    // Register proxy on account1's PXE
    register_contract_on_pxe(base.account1_wallet.pxe(), &proxy_artifact, &proxy_instance).await;

    Some(TestState {
        base,
        proxy_address,
        proxy_artifact,
    })
}

// ===========================================================================
// Tests: e2e_token_contract transfer_to_public (unshielding)
// ===========================================================================

/// TS: on behalf of self
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_public_on_behalf_of_self() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let priv_before = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = priv_before / 2;
    assert!(amount > 0, "admin should have a positive private balance");

    let pub_before = public_balance(
        &*s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;

    send_token_method(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_to_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(0), // nonce = 0 for self
        ],
        s.base.admin_address,
    )
    .await;

    wait_for_next_block(&*s.base.admin_wallet).await;

    let priv_after = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    assert_eq!(
        priv_after,
        priv_before - amount,
        "admin private balance should decrease by {amount}"
    );

    let pub_after = public_balance(
        &*s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    assert_eq!(
        pub_after,
        pub_before + amount,
        "admin public balance should increase by {amount}"
    );
}

/// TS: on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_public_on_behalf_of_other() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let priv_before = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = priv_before / 2;
    let authwit_nonce = Fr::random();
    assert!(amount > 0, "admin should have a positive private balance");

    let account1_pub_before = public_balance(
        &*s.base.admin_wallet,
        s.base.token_address,
        &s.base.account1_address,
    )
    .await;

    // Build the transfer_to_public action
    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_to_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Create authwit: admin authorizes proxy to call transfer_to_public
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
        .expect("send transfer_to_public via proxy");

    wait_for_next_block(&*s.base.admin_wallet).await;

    let priv_after = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    assert_eq!(
        priv_after,
        priv_before - amount,
        "admin private balance should decrease"
    );

    let account1_pub_after = public_balance(
        &*s.base.admin_wallet,
        s.base.token_address,
        &s.base.account1_address,
    )
    .await;
    assert_eq!(
        account1_pub_after,
        account1_pub_before + amount,
        "account1 public balance should increase"
    );

    // Perform the transfer again — should fail (duplicate nullifier)
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
            || err_str.contains("duplicate siloed nullifier")
            || err_str.contains("nullifier already exists")
            || err_str.contains("nullifier collision")
            || err_str.contains("existing nullifier"),
        "expected duplicate nullifier error, got: {err}"
    );
}

// -- failure cases --

/// TS: failure cases > on behalf of self (more than balance)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_public_on_behalf_of_self_more_than_balance() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let priv_balance = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = priv_balance + 1;
    assert!(amount > 0);

    let call = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_to_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(0),
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
        .expect_err("should fail: balance too low");

    let err_str = err.to_string();
    assert!(
        err_str.contains("Balance too low")
            || err_str.contains("Assertion failed")
            || err_str.contains("Cannot satisfy constraint"),
        "expected 'Balance too low' or constraint failure, got: {err}"
    );
}

/// TS: failure cases > on behalf of self (invalid authwit nonce)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_public_on_behalf_of_self_invalid_authwit_nonce() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let priv_balance = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = priv_balance + 1;
    assert!(amount > 0);

    let call = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_to_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(1), // non-zero nonce when from == msg_sender
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
        .expect_err("should fail: invalid authwit nonce");

    let err_str = err.to_string();
    assert!(
        err_str.contains("Invalid authwit nonce")
            || err_str.contains("Assertion failed")
            || err_str.contains("Cannot satisfy constraint"),
        "expected 'Invalid authwit nonce' or constraint failure, got: {err}"
    );
}

/// TS: failure cases > on behalf of other (more than balance)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_public_on_behalf_of_other_more_than_balance() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let priv_balance = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = priv_balance + 2;
    let authwit_nonce = Fr::random();
    assert!(amount > 0);

    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_to_public",
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

    // Admin sends through proxy — simulate only
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
}

/// TS: failure cases > on behalf of other (invalid designated caller)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_public_on_behalf_of_other_invalid_designated_caller() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let priv_balance = call_utility_u128(
        &*s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = priv_balance + 2;
    let authwit_nonce = Fr::random();
    assert!(amount > 0);

    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "transfer_to_public",
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

    // Admin sends through proxy — authwit is for wrong caller
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
}
