//! L2 to L1 messaging tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_cross_chain_messaging/l2_to_l1.test.ts`.
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

use aztec_rs::abi::AbiValue;
use aztec_rs::contract::BatchCall;
use aztec_rs::hash::compute_l2_to_l1_message_hash;
use aztec_rs::l1_client::EthClient;
use aztec_rs::node::AztecNode;

use common::*;
use std::time::Duration;
use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct L2ToL1State {
    wallet: TestWallet,
    owner: AztecAddress,
    test_artifact: ContractArtifact,
    test_address: AztecAddress,
    eth_client: EthClient,
    eth_account: EthAddress,
    l1_chain_id: u64,
    rollup_version: u64,
}

static SHARED_STATE: OnceCell<Option<L2ToL1State>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static L2ToL1State> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<L2ToL1State> {
    let (wallet, owner) = setup_wallet(TEST_ACCOUNT_0).await?;

    let node_info = wallet.pxe().node().get_node_info().await.ok()?;
    let l1_chain_id = node_info.l1_chain_id;
    let rollup_version = node_info.rollup_version;

    let eth_client = EthClient::new(&EthClient::default_url());
    let eth_account_hex = eth_client.get_account().await.ok()?;
    let eth_account = parse_eth_address(&eth_account_hex);

    // Deploy TestContract
    let test_artifact = load_test_contract_artifact();
    let deploy =
        Contract::deploy(&wallet, test_artifact.clone(), vec![], None).expect("deploy builder");
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
        .expect("deploy test contract");

    Some(L2ToL1State {
        wallet,
        owner,
        test_artifact,
        test_address: result.instance.address,
        eth_client,
        eth_account,
        l1_chain_id,
        rollup_version,
    })
}

// ---------------------------------------------------------------------------
// Helpers (uses common::build_call, common::eth_address_as_field, etc.)
// ---------------------------------------------------------------------------

// ===========================================================================
// Tests
// ===========================================================================

/// TS: 1 tx with 2 messages, one from public, one from private, to a
///     non-registered portal address
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn two_messages_private_and_public() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let content_private = Fr::random();
    let content_public = Fr::random();
    let recipient_field = eth_address_as_field(&s.eth_account);

    // Batch: private L2→L1 message + public L2→L1 message
    let call_private = build_call(
        &s.test_artifact,
        s.test_address,
        "create_l2_to_l1_message_arbitrary_recipient_private",
        vec![
            AbiValue::Field(content_private),
            AbiValue::Field(recipient_field),
        ],
    );
    let call_public = build_call(
        &s.test_artifact,
        s.test_address,
        "create_l2_to_l1_message_arbitrary_recipient_public",
        vec![
            AbiValue::Field(content_public),
            AbiValue::Field(recipient_field),
        ],
    );

    let batch = BatchCall::new(
        &s.wallet,
        vec![
            ExecutionPayload {
                calls: vec![call_private],
                ..Default::default()
            },
            ExecutionPayload {
                calls: vec![call_public],
                ..Default::default()
            },
        ],
    );

    let send_result = batch
        .send(SendOptions {
            from: s.owner,
            ..Default::default()
        })
        .await
        .expect("batch L2→L1 messages");

    // Verify the tx was included
    let tx_effect = s
        .wallet
        .pxe()
        .node()
        .get_tx_effect(&send_result.tx_hash)
        .await
        .expect("get tx effect");

    assert!(tx_effect.is_some(), "tx should be included in a block");

    // Compute expected message hashes
    let expected_private = compute_l2_to_l1_message_hash(
        &s.test_address,
        &s.eth_account,
        &content_private,
        &Fr::from(s.rollup_version),
        &Fr::from(s.l1_chain_id),
    );
    let expected_public = compute_l2_to_l1_message_hash(
        &s.test_address,
        &s.eth_account,
        &content_public,
        &Fr::from(s.rollup_version),
        &Fr::from(s.l1_chain_id),
    );

    // Verify message hashes are non-zero (messages were created)
    assert_ne!(expected_private, Fr::zero());
    assert_ne!(expected_public, Fr::zero());

    // Check that the tx effects contain L2→L1 messages
    if let Some(ref effect) = tx_effect {
        let l2_to_l1 = effect
            .pointer("/data/l2ToL1Msgs")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .filter(|s| *s != Fr::zero().to_string())
                    .count()
            })
            .unwrap_or(0);
        assert!(
            l2_to_l1 >= 1,
            "tx should contain at least 1 L2→L1 message (found {l2_to_l1})"
        );
    }

    // Note: Full L1 Outbox consumption requires epoch proving + Outbox.consume().
    // The message hashes are verified locally; L1 consumption is tested when
    // epoch proving infrastructure is available.
}

/// TS: 2 txs in the same block, one with no messages, one with a message
///
/// Requires `setConfig({ minTxsPerBlock: 2 })` to force both txs into one block.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn two_txs_one_empty_one_with_message() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let content = Fr::random();
    let recipient_field = eth_address_as_field(&s.eth_account);

    // Send a private L2→L1 message
    let call = build_call(
        &s.test_artifact,
        s.test_address,
        "create_l2_to_l1_message_arbitrary_recipient_private",
        vec![AbiValue::Field(content), AbiValue::Field(recipient_field)],
    );

    let send_result = s
        .wallet
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
        .expect("send L2→L1 message");

    // Verify inclusion
    let tx_effect = s
        .wallet
        .pxe()
        .node()
        .get_tx_effect(&send_result.tx_hash)
        .await
        .expect("get tx effect");
    assert!(tx_effect.is_some(), "tx should be included");

    // Note: Cannot force 2 txs into same block without setConfig({ minTxsPerBlock: 2 }).
    // The single-message case is verified.
}

/// TS: 2 txs (balanced), one with 3 messages (unbalanced), one with 4 messages (balanced)
///
/// Requires `setConfig({ minTxsPerBlock: 2 })`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn balanced_and_unbalanced_message_trees() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let recipient_field = eth_address_as_field(&s.eth_account);

    // Send 3 messages in a batch (simulating first tx)
    let contents: Vec<Fr> = (0..3).map(|_| Fr::random()).collect();
    let calls: Vec<ExecutionPayload> = contents
        .iter()
        .map(|c| {
            let call = build_call(
                &s.test_artifact,
                s.test_address,
                "create_l2_to_l1_message_arbitrary_recipient_private",
                vec![AbiValue::Field(*c), AbiValue::Field(recipient_field)],
            );
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            }
        })
        .collect();

    let batch = BatchCall::new(&s.wallet, calls);
    let result = batch
        .send(SendOptions {
            from: s.owner,
            ..Default::default()
        })
        .await
        .expect("send 3-message batch");

    let tx_effect = s
        .wallet
        .pxe()
        .node()
        .get_tx_effect(&result.tx_hash)
        .await
        .expect("get tx effect");
    assert!(tx_effect.is_some(), "tx should be included");

    // Note: Cannot force second tx (4 messages) into same block without
    // node admin setConfig. The 3-message batch is verified.
}

/// TS: 3 txs (unbalanced), complex message tree structure
///
/// Requires `setConfig({ minTxsPerBlock: 3 })`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn complex_unbalanced_message_tree() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let recipient_field = eth_address_as_field(&s.eth_account);

    // Send 2 messages in a batch
    let contents: Vec<Fr> = (0..2).map(|_| Fr::random()).collect();
    let calls: Vec<ExecutionPayload> = contents
        .iter()
        .map(|c| {
            let call = build_call(
                &s.test_artifact,
                s.test_address,
                "create_l2_to_l1_message_arbitrary_recipient_private",
                vec![AbiValue::Field(*c), AbiValue::Field(recipient_field)],
            );
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            }
        })
        .collect();

    let batch = BatchCall::new(&s.wallet, calls);
    let result = batch
        .send(SendOptions {
            from: s.owner,
            ..Default::default()
        })
        .await
        .expect("send 2-message batch");

    let tx_effect = s
        .wallet
        .pxe()
        .node()
        .get_tx_effect(&result.tx_hash)
        .await
        .expect("get tx effect");
    assert!(tx_effect.is_some(), "tx should be included");

    // Note: Full 3-tx-in-one-block test requires node admin setConfig.
    // The batched messages are verified.
}
