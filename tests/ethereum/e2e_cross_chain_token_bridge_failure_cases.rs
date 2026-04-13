//! Token bridge failure cases — 1:1 mirror of upstream
//! `end-to-end/src/e2e_cross_chain_messaging/token_bridge_failure_cases.test.ts`.
//!
//! The upstream tests use a dedicated TokenPortal + TokenBridge contract pair.
//! Since those require custom L1 contract deployment, we emulate the same
//! failure patterns using the sandbox's pre-deployed contracts and
//! TestContract messaging:
//!   * Authorization failure on a public burn-style path (no authwit) →
//!     mirrors "Bridge can't withdraw my funds if I don't give approval".
//!   * Wrong content / claim-kind mismatch → mirrors "Can't claim funds
//!     privately which were intended for public deposit" and vice versa.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_cross_chain_token_bridge_failure_cases -- --ignored --nocapture
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

use aztec_rs::abi::AbiValue;
use aztec_rs::cross_chain;
use aztec_rs::l1_client::{self, EthClient, L1ContractAddresses};
use aztec_rs::messaging;
use aztec_rs::node::AztecNode;

use crate::common::*;
use std::time::Duration;
use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct FailureCasesState {
    token: TokenTestState,
    eth_client: EthClient,
    l1_addresses: L1ContractAddresses,
    rollup_version: u64,
    test_artifact: ContractArtifact,
    test_address: AztecAddress,
}

static SHARED_STATE: OnceCell<Option<FailureCasesState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static FailureCasesState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<FailureCasesState> {
    // Two wallets (admin + account1) with a token contract holding a public
    // balance on admin — mirrors upstream `owner` + `user1` setup.
    let token = init_token_test_state(/* public_mint */ 100, /* private_mint */ 0).await?;

    let node_info = token.admin_wallet.pxe().node().get_node_info().await.ok()?;
    let l1_addresses = L1ContractAddresses::from_json(&node_info.l1_contract_addresses)?;
    let rollup_version = node_info.rollup_version;

    let eth_client = EthClient::new(&EthClient::default_url());
    eth_client.get_account().await.ok()?;

    // Deploy TestContract for L1↔L2 message plumbing (shared across tests).
    let test_artifact = load_test_contract_artifact();
    let deploy_test = Contract::deploy(&token.admin_wallet, test_artifact.clone(), vec![], None)
        .expect("deploy builder");
    let test_result = deploy_test
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: token.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect("deploy test contract");

    Some(FailureCasesState {
        token,
        eth_client,
        l1_addresses,
        rollup_version,
        test_artifact,
        test_address: test_result.instance.address,
    })
}

/// Send empty tx to advance the L2 block number.
async fn advance_block(wallet: &TestWallet, from: AztecAddress) {
    wallet
        .send_tx(
            ExecutionPayload::default(),
            SendOptions {
                from,
                ..Default::default()
            },
        )
        .await
        .ok();
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: Bridge can't withdraw my funds if I don't give approval
///
/// Upstream: owner mints tokens publicly on L2, then user1 tries to
/// `exit_to_l1_public` on the owner's balance without an authwit — should
/// revert with "unauthorized". We emulate the same authorization pattern by
/// having account1 attempt a `burn_public` on admin's public balance with a
/// non-zero authwit nonce that was never approved.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn bridge_cannot_withdraw_without_approval() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let withdraw_amount = 9i128;
    let authwit_nonce = Fr::random();

    // account1 attempts to burn admin's balance without any authwit approval.
    // This is exactly the authorization edge that TokenBridge.exit_to_l1_public
    // relies on (bridge must be pre-authorised to burn the user's balance).
    let err = s
        .token
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &s.token.token_artifact,
                    s.token.token_address,
                    "burn_public",
                    vec![
                        AbiValue::Field(Fr::from(s.token.admin_address)),
                        AbiValue::Integer(withdraw_amount),
                        AbiValue::Field(authwit_nonce),
                    ],
                )],
                ..Default::default()
            },
            SimulateOptions {
                from: s.token.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("public burn without authwit should be rejected");

    assert!(
        err.to_string().to_lowercase().contains("unauthorized"),
        "expected an 'unauthorized' failure, got: {err}"
    );
}

/// TS: Can't claim funds privately which were intended for public deposit
///     from the token portal
///
/// Upstream: a valid L1→L2 message is sent for a public deposit, but the
/// claim is attempted with a different bridge amount → the computed message
/// hash diverges and the L2 circuit rejects with
/// "No L1 to L2 message found for message hash ...". We reproduce the
/// divergent-message-hash path directly using TestContract: consume a real
/// message but pass a content value that does not match what was inserted.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn cannot_claim_private_with_wrong_message_hash() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // 1. Send a real L1→L2 message.
    let (secret, secret_hash) = messaging::generate_claim_secret();
    let content = Fr::random();

    let msg_result = l1_client::send_l1_to_l2_message(
        &s.eth_client,
        &s.l1_addresses.inbox,
        &s.test_address,
        s.rollup_version,
        &content,
        &secret_hash,
    )
    .await
    .expect("send L1→L2 message");

    // 2. Wait for readiness.
    for _ in 0..30 {
        advance_block(&s.token.admin_wallet, s.token.admin_address).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        if cross_chain::is_l1_to_l2_message_ready(
            s.token.admin_wallet.pxe().node(),
            &msg_result.msg_hash,
        )
        .await
        .unwrap_or(false)
        {
            break;
        }
    }

    // 3. Attempt to consume with WRONG content — the circuit will compute a
    //    different message hash and fail to find it in the L1→L2 tree.
    let eth_account_hex = s.eth_client.get_account().await.expect("get L1 account");
    let eth_addr_fr = eth_address_as_field(&parse_eth_address(&eth_account_hex));
    // Pick a value that cannot equal `content` (random is negligible-collision).
    let wrong_content = Fr::random();

    let consume_wrong = build_call(
        &s.test_artifact,
        s.test_address,
        "consume_message_from_arbitrary_sender_private",
        vec![
            AbiValue::Field(wrong_content),
            AbiValue::Field(secret),
            AbiValue::Field(eth_addr_fr),
            AbiValue::Field(msg_result.global_leaf_index),
        ],
    );

    let result = s
        .token
        .admin_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![consume_wrong],
                ..Default::default()
            },
            SendOptions {
                from: s.token.admin_address,
                ..Default::default()
            },
        )
        .await;

    match result {
        Ok(_) => panic!("consuming with wrong content should be rejected"),
        Err(err) => {
            let err_str = err.to_string();
            // Mirrors upstream `NO_L1_TO_L2_MSG_ERROR` regex plus circuit-level
            // membership/constraint failures that can surface the same root
            // cause when the content-derived hash misses the tree.
            assert!(
                err_str.contains("No non-nullified L1 to L2 message found for message hash")
                    || err_str.contains("Tried to consume nonexistent L1-to-L2 message")
                    || err_str.contains("No L1 to L2 message found")
                    || err_str.contains("membership")
                    || err_str.contains("Cannot satisfy constraint"),
                "expected message-hash mismatch failure, got: {err}"
            );
        }
    }
}

/// TS: Can't claim funds publicly which were intended for private deposit
///     from the token portal
///
/// Upstream: a private deposit (`mint_to_private`) is posted with a specific
/// claim secret hash, and a subsequent call to `claim_public` with a random
/// (wrong) secret fails with `NO_L1_TO_L2_MSG_ERROR`. We reproduce the
/// wrong-secret rejection directly against a live L1→L2 message.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn cannot_claim_public_with_wrong_secret() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // 1. Send a real L1→L2 message bound to `secret_hash`.
    let (_secret, secret_hash) = messaging::generate_claim_secret();
    let content = Fr::random();

    let msg_result = l1_client::send_l1_to_l2_message(
        &s.eth_client,
        &s.l1_addresses.inbox,
        &s.test_address,
        s.rollup_version,
        &content,
        &secret_hash,
    )
    .await
    .expect("send L1→L2 message");

    // 2. Wait for readiness.
    for _ in 0..30 {
        advance_block(&s.token.admin_wallet, s.token.admin_address).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        if cross_chain::is_l1_to_l2_message_ready(
            s.token.admin_wallet.pxe().node(),
            &msg_result.msg_hash,
        )
        .await
        .unwrap_or(false)
        {
            break;
        }
    }

    // 3. Attempt to consume publicly using a random (wrong) secret — the
    //    nullifier + hash computation will diverge, so the message is not
    //    found.
    let eth_account_hex = s.eth_client.get_account().await.expect("get L1 account");
    let eth_addr_fr = eth_address_as_field(&parse_eth_address(&eth_account_hex));
    let wrong_secret = Fr::random();

    let consume_public_wrong = build_call(
        &s.test_artifact,
        s.test_address,
        "consume_message_from_arbitrary_sender_public",
        vec![
            AbiValue::Field(content),
            AbiValue::Field(wrong_secret),
            AbiValue::Field(eth_addr_fr),
            AbiValue::Field(msg_result.global_leaf_index),
        ],
    );

    let result = s
        .token
        .admin_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![consume_public_wrong],
                ..Default::default()
            },
            SendOptions {
                from: s.token.admin_address,
                ..Default::default()
            },
        )
        .await;

    match result {
        Ok(_) => panic!("public consume with wrong secret should be rejected"),
        Err(err) => {
            let err_str = err.to_string();
            // Mirrors upstream `NO_L1_TO_L2_MSG_ERROR` regex (either phrasing is
            // accepted). Circuit-level failures (membership / unsatisfied
            // constraint / nullifier checks) are included because the same
            // wrong-secret condition can surface through those paths.
            assert!(
                err_str.contains("No non-nullified L1 to L2 message found for message hash")
                    || err_str.contains("Tried to consume nonexistent L1-to-L2 message")
                    || err_str.contains("No L1 to L2 message found")
                    || err_str.contains("membership")
                    || err_str.contains("Cannot satisfy constraint")
                    || err_str.contains("nullifier"),
                "expected wrong-secret failure, got: {err}"
            );
        }
    }
}
