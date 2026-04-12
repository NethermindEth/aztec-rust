//! Token burn tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/burn.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_burn -- --ignored --nocapture
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

mod common;
use common::*;

use aztec_rs::authwit::SetPublicAuthWitInteraction;
use aztec_rs::hash::{compute_auth_wit_message_hash, MessageHashOrIntent};

const DUPLICATE_NULLIFIER_ERROR: &str = "nullifier";
const MINT_AMOUNT: u64 = 10000;

// ---------------------------------------------------------------------------
// Shared test state
// ---------------------------------------------------------------------------

struct TestState {
    base: AuthwitTokenTestState,
    proxy_address: AztecAddress,
    proxy_artifact: ContractArtifact,
}

static SHARED_STATE: OnceCell<Option<TestState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static TestState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<TestState> {
    let base = init_authwit_token_test_state(MINT_AMOUNT, MINT_AMOUNT).await?;

    let (proxy_address, proxy_artifact, proxy_instance) = deploy_contract(
        &*base.admin_wallet,
        load_generic_proxy_artifact(),
        vec![],
        base.admin_address,
    )
    .await;

    register_contract_on_pxe(base.account1_wallet.pxe(), &proxy_artifact, &proxy_instance).await;

    Some(TestState {
        base,
        proxy_address,
        proxy_artifact,
    })
}

// ---------------------------------------------------------------------------
// Test-specific helpers (not in common)
// ---------------------------------------------------------------------------

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
// Tests: Public burn
// ---------------------------------------------------------------------------

/// TS: public > burn less than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_burn_less_than_balance() {
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

    send_token_method(
        &s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "burn_public",
        vec![
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
    assert_eq!(balance_after, balance0 - amount);
}

/// TS: public > burn on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_burn_on_behalf_of_other() {
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
    let authwit_nonce = Fr::random();
    assert!(amount > 0);

    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "burn_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
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
    .expect("create public authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send public authwit");

    s.base
        .account1_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![action.clone()],
                ..Default::default()
            },
            SendOptions {
                from: s.base.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect("burn public on behalf");

    wait_for_next_block(&s.base.account1_wallet).await;
    let balance_after = public_balance(
        &s.base.admin_wallet,
        s.base.token_address,
        &s.base.admin_address,
    )
    .await;
    assert_eq!(balance_after, balance0 - amount);

    let replay_err = s
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
        .expect_err("replay should fail");
    assert!(replay_err
        .to_string()
        .to_lowercase()
        .contains("unauthorized"));
}

/// TS: public > burn more than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_burn_more_than_balance_fails() {
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
    let err = s
        .base
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &s.base.token_artifact,
                    s.base.token_address,
                    "burn_public",
                    vec![
                        AbiValue::Field(Fr::from(s.base.admin_address)),
                        AbiValue::Integer(amount as i128),
                        AbiValue::Integer(0),
                    ],
                )],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("burn more than balance should fail");
    assert!(err.to_string().contains(U128_UNDERFLOW_ERROR));
}

/// TS: public > burn on behalf of self with non-zero nonce
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_burn_self_nonzero_nonce_fails() {
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
    let amount = balance0 - 1;
    let err = s
        .base
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &s.base.token_artifact,
                    s.base.token_address,
                    "burn_public",
                    vec![
                        AbiValue::Field(Fr::from(s.base.admin_address)),
                        AbiValue::Integer(amount as i128),
                        AbiValue::Integer(1),
                    ],
                )],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("non-zero nonce on self burn should fail");
    assert!(err.to_string().contains("Invalid authwit nonce"));
}

/// TS: public > burn on behalf of other without "approval"
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_burn_on_behalf_without_approval_fails() {
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
    let err = s
        .base
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &s.base.token_artifact,
                    s.base.token_address,
                    "burn_public",
                    vec![
                        AbiValue::Field(Fr::from(s.base.admin_address)),
                        AbiValue::Integer(amount as i128),
                        AbiValue::Field(authwit_nonce),
                    ],
                )],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("missing approval should fail");
    assert!(err.to_string().to_lowercase().contains("unauthorized"));
}

/// TS: public > burn more than balance on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_burn_more_than_balance_on_behalf_fails() {
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
    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "burn_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
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
    .expect("create public authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send public authwit");

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
        .expect_err("underflow should fail");
    assert!(err.to_string().contains(U128_UNDERFLOW_ERROR));
}

/// TS: public > burn on behalf of other, wrong designated caller
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_burn_wrong_designated_caller_fails() {
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
    let amount = balance0 + 2;
    let authwit_nonce = Fr::random();
    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "burn_public",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
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
    .expect("create wrong public authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send wrong public authwit");

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
        .expect_err("wrong caller should fail");
    assert!(err.to_string().to_lowercase().contains("unauthorized"));
}

// ---------------------------------------------------------------------------
// Tests: Private burn
// ---------------------------------------------------------------------------

/// TS: private > burn less than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_burn_less_than_balance() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    assert!(amount > 0);

    send_token_method(
        &s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "burn_private",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(0),
        ],
        s.base.admin_address,
    )
    .await;

    let balance_after = call_utility_u128(
        &s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    assert_eq!(balance_after, balance0 - amount);
}

/// TS: private > burn on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_burn_on_behalf_of_other() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.base.admin_wallet,
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
        "burn_private",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
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
        .expect("burn through authwit proxy");

    let balance_after = call_utility_u128(
        &s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    assert_eq!(balance_after, balance0 - amount);

    let replay_err = s
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
        .expect_err("duplicate nullifier replay should fail");
    assert!(replay_err
        .to_string()
        .to_lowercase()
        .contains(DUPLICATE_NULLIFIER_ERROR));
}

/// TS: private > burn more than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_burn_more_than_balance_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let err = s
        .base
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &s.base.token_artifact,
                    s.base.token_address,
                    "burn_private",
                    vec![
                        AbiValue::Field(Fr::from(s.base.admin_address)),
                        AbiValue::Integer((balance0 + 1) as i128),
                        AbiValue::Integer(0),
                    ],
                )],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("burn more than balance should fail");
    assert!(err.to_string().contains("Balance too low"));
}

/// TS: private > burn on behalf of self with non-zero nonce
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_burn_self_nonzero_nonce_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let err = s
        .base
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &s.base.token_artifact,
                    s.base.token_address,
                    "burn_private",
                    vec![
                        AbiValue::Field(Fr::from(s.base.admin_address)),
                        AbiValue::Integer((balance0 - 1) as i128),
                        AbiValue::Integer(1),
                    ],
                )],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("non-zero nonce on self burn should fail");
    assert!(err.to_string().contains("Invalid authwit nonce"));
}

/// TS: private > burn more than balance on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_burn_more_than_balance_on_behalf_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = balance0 + 1;
    let authwit_nonce = Fr::random();
    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "burn_private",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let witness = s
        .base
        .admin_wallet
        .create_auth_wit(
            s.base.admin_address,
            MessageHashOrIntent::Intent {
                caller: s.proxy_address,
                call: action.clone(),
            },
        )
        .await
        .expect("create authwit");

    let err = s
        .base
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_proxy_call(
                    &s.proxy_artifact,
                    s.proxy_address,
                    &action,
                )],
                auth_witnesses: vec![witness],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("balance too low on behalf should fail");
    assert!(err.to_string().contains("Balance too low"));
}

/// TS: private > burn on behalf of other without approval
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_burn_without_approval_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    let authwit_nonce = Fr::random();
    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "burn_private",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let message_hash = compute_auth_wit_message_hash(
        &MessageHashOrIntent::Intent {
            caller: s.proxy_address,
            call: action.clone(),
        },
        &s.base
            .admin_wallet
            .get_chain_info()
            .await
            .expect("chain info"),
    );

    let err = s
        .base
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_proxy_call(
                    &s.proxy_artifact,
                    s.proxy_address,
                    &action,
                )],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("missing authwit should fail");
    assert!(
        err.to_string().contains(&format!(
            "Unknown auth witness for message hash {message_hash}"
        )) || err.to_string().contains("Unknown auth witness")
    );
}

/// TS: private > on behalf of other (invalid designated caller)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_burn_invalid_designated_caller_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.base.admin_wallet,
        &s.base.token_artifact,
        s.base.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.base.admin_address))],
        s.base.admin_address,
    )
    .await;
    let amount = balance0 + 2;
    let authwit_nonce = Fr::random();
    let action = build_call(
        &s.base.token_artifact,
        s.base.token_address,
        "burn_private",
        vec![
            AbiValue::Field(Fr::from(s.base.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let expected_hash = compute_auth_wit_message_hash(
        &MessageHashOrIntent::Intent {
            caller: s.proxy_address,
            call: action.clone(),
        },
        &s.base
            .admin_wallet
            .get_chain_info()
            .await
            .expect("chain info"),
    );
    let witness = s
        .base
        .admin_wallet
        .create_auth_wit(
            s.base.admin_address,
            MessageHashOrIntent::Intent {
                caller: s.base.account1_address,
                call: action.clone(),
            },
        )
        .await
        .expect("create mismatched authwit");

    let err = s
        .base
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_proxy_call(
                    &s.proxy_artifact,
                    s.proxy_address,
                    &action,
                )],
                auth_witnesses: vec![witness],
                ..Default::default()
            },
            SimulateOptions {
                from: s.base.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("wrong designated caller should fail");
    assert!(
        err.to_string().contains(&format!(
            "Unknown auth witness for message hash {expected_hash}"
        )) || err.to_string().contains("Unknown auth witness")
    );
}
