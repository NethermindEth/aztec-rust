//! Integration tests for aztec-rs against a local Aztec node.
//!
//! These tests are `#[ignore]`d by default because they require a running
//! Aztec node. Run them with:
//!
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test integration -- --ignored
//! ```

#![allow(clippy::expect_used, clippy::print_stderr)]

use aztec_rs::abi::{AbiValue, ContractArtifact};
use aztec_rs::contract::Contract;
use aztec_rs::node::{create_aztec_node_client, wait_for_node, AztecNode, NodeInfo, WaitOpts};
use aztec_rs::tx::TxHash;
use aztec_rs::types::{AztecAddress, Fr};
use aztec_rs::wallet::{ChainInfo, MockWallet, SimulateOptions};
use std::time::Duration;

fn node_url() -> String {
    std::env::var("AZTEC_NODE_URL").unwrap_or_else(|_| "http://localhost:8080".to_owned())
}

async fn require_live_node() -> Option<(impl AztecNode, NodeInfo)> {
    let node = create_aztec_node_client(node_url());
    let info = match wait_for_node(&node).await {
        Ok(info) => info,
        Err(err) => {
            eprintln!("skipping ignored integration test: unable to reach Aztec node: {err}");
            return None;
        }
    };

    Some((node, info))
}

// ---------------------------------------------------------------------------
// Fixture loading tests
// ---------------------------------------------------------------------------

#[test]
fn load_token_contract_fixture() {
    let json = include_str!("../fixtures/token_contract.json");
    let artifact = ContractArtifact::from_json(json).expect("parse token_contract.json");
    assert_eq!(artifact.name, "TokenContract");
    assert_eq!(artifact.functions.len(), 11);

    let constructor = artifact
        .find_function("constructor")
        .expect("find constructor");
    assert!(constructor.is_initializer);
    assert_eq!(constructor.parameters.len(), 4);

    let transfer = artifact.find_function("transfer").expect("find transfer");
    assert!(!transfer.is_initializer);
    assert!(!transfer.is_static);
    assert_eq!(transfer.parameters.len(), 4);

    let balance = artifact
        .find_function("balance_of_public")
        .expect("find balance_of_public");
    assert!(balance.is_static);
    assert_eq!(balance.return_types.len(), 1);

    let total_supply = artifact
        .find_function("total_supply")
        .expect("find total_supply");
    assert!(total_supply.is_static);
    assert!(total_supply.parameters.is_empty());
}

#[test]
fn load_counter_contract_fixture() {
    let json = include_str!("../fixtures/counter_contract.json");
    let artifact = ContractArtifact::from_json(json).expect("parse counter_contract.json");
    assert_eq!(artifact.name, "CounterContract");
    assert_eq!(artifact.functions.len(), 3);

    let constructor = artifact
        .find_function("constructor")
        .expect("find constructor");
    assert!(constructor.is_initializer);
    assert_eq!(constructor.parameters.len(), 2);

    let increment = artifact.find_function("increment").expect("find increment");
    assert!(!increment.is_initializer);

    let get_counter = artifact
        .find_function("get_counter")
        .expect("find get_counter");
    assert!(get_counter.is_static);
}

#[test]
fn load_escrow_contract_fixture() {
    let json = include_str!("../fixtures/escrow_contract.json");
    let artifact = ContractArtifact::from_json(json).expect("parse escrow_contract.json");
    assert_eq!(artifact.name, "EscrowContract");
    assert_eq!(artifact.functions.len(), 4);
}

#[test]
fn fixture_contract_interaction_with_mock_wallet() {
    let json = include_str!("../fixtures/token_contract.json");
    let artifact = ContractArtifact::from_json(json).expect("parse token_contract.json");

    let wallet = MockWallet::new(ChainInfo {
        chain_id: Fr::from(31337u64),
        version: Fr::from(1u64),
    });

    let contract = Contract::at(AztecAddress(Fr::from(42u64)), artifact, wallet);

    // Build a transfer interaction from the fixture
    let interaction = contract
        .method(
            "transfer",
            vec![
                AbiValue::Field(Fr::from(1u64)),
                AbiValue::Field(Fr::from(2u64)),
                AbiValue::Integer(100),
                AbiValue::Field(Fr::from(0u64)),
            ],
        )
        .expect("build transfer interaction");

    let payload = interaction.request().expect("build payload");
    assert_eq!(payload.calls.len(), 1);
    assert_eq!(payload.calls[0].selector.to_string(), "0xd6f42325");
}

// ---------------------------------------------------------------------------
// Node connectivity tests (require live node)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a reachable Aztec node via AZTEC_NODE_URL"]
async fn connect_to_node() {
    let Some((node, _)) = require_live_node().await else {
        return;
    };
    let info = node
        .get_node_info()
        .await
        .expect("should connect to the node");
    assert!(!info.node_version.is_empty(), "node version should be set");
    assert!(info.l1_chain_id > 0, "L1 chain ID should be positive");
}

#[tokio::test]
#[ignore = "requires a reachable Aztec node via AZTEC_NODE_URL"]
async fn wait_for_node_readiness() {
    let Some((_, info)) = require_live_node().await else {
        return;
    };
    assert!(
        !info.node_version.is_empty(),
        "node should report a version"
    );
}

#[tokio::test]
#[ignore = "requires a reachable Aztec node via AZTEC_NODE_URL"]
async fn get_block_number() {
    let Some((node, _)) = require_live_node().await else {
        return;
    };

    let block_number = node
        .get_block_number()
        .await
        .expect("should get block number");
    // Block number should be a reasonable value (0 is valid for fresh nodes)
    assert!(
        block_number < 1_000_000_000,
        "block number should be reasonable"
    );
}

#[tokio::test]
#[ignore = "requires a reachable Aztec node via AZTEC_NODE_URL"]
async fn get_tx_receipt_for_unknown_hash() {
    let Some((node, _)) = require_live_node().await else {
        return;
    };

    // A random tx hash should not be found or return a dropped/pending status
    let hash =
        TxHash::from_hex("0x00000000000000000000000000000000000000000000000000000000deadbeef")
            .expect("valid hex");

    // The node may return an error or a receipt with a non-success status
    let result = node.get_tx_receipt(&hash).await;
    if let Ok(receipt) = result {
        // If the node returns a receipt for an unknown hash, it should not
        // be in a successful terminal state
        assert!(
            !receipt.has_execution_succeeded(),
            "unknown tx should not show as succeeded"
        );
    }
}

#[tokio::test]
#[ignore = "requires a reachable Aztec node via AZTEC_NODE_URL"]
async fn get_public_logs_with_empty_filter() {
    use aztec_rs::node::PublicLogFilter;

    let Some((node, _)) = require_live_node().await else {
        return;
    };

    let response = node
        .get_public_logs(PublicLogFilter::default())
        .await
        .expect("should get public logs");

    // With no filter, we might get logs or an empty response — both are valid
    assert!(response.logs.len() <= 1000, "log count should be bounded");
}

#[tokio::test]
#[ignore = "requires a reachable Aztec node via AZTEC_NODE_URL"]
async fn wait_for_tx_timeout_on_unknown_hash() {
    use aztec_rs::node::wait_for_tx;

    let Some((node, _)) = require_live_node().await else {
        return;
    };

    let hash =
        TxHash::from_hex("0x0000000000000000000000000000000000000000000000000000000000abcdef")
            .expect("valid hex");

    let opts = WaitOpts {
        timeout: Duration::from_secs(3),
        interval: Duration::from_secs(1),
        ..WaitOpts::default()
    };

    let result = wait_for_tx(&node, &hash, opts).await;
    assert!(
        result.is_err(),
        "waiting for unknown tx should timeout or error"
    );
}

// ---------------------------------------------------------------------------
// Node info structure tests (require live node)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a reachable Aztec node via AZTEC_NODE_URL"]
async fn node_info_has_expected_fields() {
    let Some((_, info)) = require_live_node().await else {
        return;
    };

    // Verify the structure matches what we expect
    assert!(
        info.rollup_version >= 1,
        "rollup version should be at least 1"
    );

    // L1 contract addresses should be present (even if empty object)
    assert!(
        info.l1_contract_addresses.is_object() || info.l1_contract_addresses.is_null(),
        "l1_contract_addresses should be object or null"
    );
}

// ---------------------------------------------------------------------------
// Contract interaction tests with fixtures (require live node)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a reachable Aztec node via AZTEC_NODE_URL"]
async fn load_fixture_and_build_contract_handle() {
    let Some((_, info)) = require_live_node().await else {
        return;
    };

    // Load the token contract fixture
    let json = include_str!("../fixtures/token_contract.json");
    let artifact = ContractArtifact::from_json(json).expect("parse token_contract.json");

    // Create a mock wallet backed by the node's chain info
    let wallet = MockWallet::new(ChainInfo {
        chain_id: Fr::from(info.l1_chain_id),
        version: Fr::from(info.rollup_version),
    });

    // Build a contract handle at a placeholder address
    let contract = Contract::at(AztecAddress(Fr::from(1u64)), artifact, wallet);

    // Verify we can build interactions for various function types
    let _balance = contract
        .method("balance_of_public", vec![AbiValue::Field(Fr::from(1u64))])
        .expect("build balance_of_public interaction");

    let _transfer = contract
        .method(
            "transfer",
            vec![
                AbiValue::Field(Fr::from(1u64)),
                AbiValue::Field(Fr::from(2u64)),
                AbiValue::Integer(100),
                AbiValue::Field(Fr::from(0u64)),
            ],
        )
        .expect("build transfer interaction");

    let _total_supply = contract
        .method("total_supply", vec![])
        .expect("build total_supply interaction");
}

#[tokio::test]
#[ignore = "requires a reachable Aztec node via AZTEC_NODE_URL"]
async fn simulate_with_mock_wallet_and_live_chain_info() {
    let Some((_, info)) = require_live_node().await else {
        return;
    };

    let json = include_str!("../fixtures/counter_contract.json");
    let artifact = ContractArtifact::from_json(json).expect("parse counter_contract.json");

    let wallet = MockWallet::new(ChainInfo {
        chain_id: Fr::from(info.l1_chain_id),
        version: Fr::from(info.rollup_version),
    });

    let contract = Contract::at(AztecAddress(Fr::from(1u64)), artifact, wallet);

    // Simulate get_counter — this uses the mock wallet, so it will return
    // the mock's default simulation result, but validates the full path
    let result = contract
        .method("get_counter", vec![AbiValue::Field(Fr::from(1u64))])
        .expect("build get_counter interaction")
        .simulate(SimulateOptions::default())
        .await
        .expect("simulate should succeed with mock wallet");

    // MockWallet returns null by default
    assert_eq!(result.return_values, serde_json::Value::Null);
}
