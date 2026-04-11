//! Pruned blocks tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_pruned_blocks.test.ts`.
//!
//! Tests PXE interacting with a node that has pruned relevant blocks,
//! preventing usage of the archive API (which PXE should not rely on).
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_pruned_blocks -- --ignored --nocapture
//! ```

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::too_many_lines,
    dead_code
)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionType};
use aztec_rs::account::{SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::{Pxe, RegisterContractRequest};
use aztec_rs::tx::{ExecutionPayload, FunctionCall};
use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
use aztec_rs::wallet::{BaseWallet, SendOptions, SendResult, Wallet};

const MINT_AMOUNT: u64 = 1000;

/// Max blocks to mine while waiting for pruning. The upstream TS test starts its
/// own node with `worldStateCheckpointHistory: 2` and only needs 5 blocks, but when
/// connecting to an external node the history window may be much larger (e.g. ~80+).
const MAX_BLOCKS_TO_MINE: usize = 120;

/// `MerkleTreeId.NOTE_HASH_TREE = 1` (upstream enum value)
const NOTE_HASH_TREE: &str = "1";

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_compiled_token_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/token_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse token_contract_compiled.json")
}

// ---------------------------------------------------------------------------
// Setup helpers (mirrors upstream fixtures/utils.ts)
// ---------------------------------------------------------------------------

type TestWallet = BaseWallet<EmbeddedPxe<HttpNodeClient>, HttpNodeClient, SingleAccountProvider>;

#[derive(Clone, Copy)]
struct ImportedTestAccount {
    alias: &'static str,
    address: &'static str,
    secret_key: &'static str,
    partial_address: &'static str,
}

const TEST_ACCOUNT_0: ImportedTestAccount = ImportedTestAccount {
    alias: "test0",
    address: "0x0a60414ee907527880b7a53d4dacdeb9ef768bb98d9d8d1e7200725c13763331",
    secret_key: "0x2153536ff6628eee01cf4024889ff977a18d9fa61d0e414422f7681cf085c281",
    partial_address: "0x140c3a658e105092549c8402f0647fe61d87aba4422b484dfac5d4a87462eeef",
};

const TEST_ACCOUNT_1: ImportedTestAccount = ImportedTestAccount {
    alias: "test1",
    address: "0x00cedf87a800bd88274762d77ffd93e97bc881d1fc99570d62ba97953597914d",
    secret_key: "0x0aebd1b4be76efa44f5ee655c20bf9ea60f7ae44b9a7fd1fd9f189c7a0b0cdae",
    partial_address: "0x0325ee1689daec508c6adef0df4a1e270ac1fcf971fed1f893b2d98ad12d6bb8",
};

const TEST_ACCOUNT_2: ImportedTestAccount = ImportedTestAccount {
    alias: "test2",
    address: "0x1dd551228da3a56b5da5f5d73728e08d8114f59897c27136f1bcdd4c05028905",
    secret_key: "0x0f6addf0da06c33293df974a565b03d1ab096090d907d98055a8b7f4954e120c",
    partial_address: "0x17604ccd69bd09d8df02c4a345bc4232e5d24b568536c55407b3e4e4e3354c4c",
};

fn node_url() -> String {
    std::env::var("AZTEC_NODE_URL").unwrap_or_else(|_| "http://localhost:8080".to_owned())
}

#[allow(clippy::cast_possible_truncation)]
fn next_unique_salt() -> u64 {
    static NEXT_SALT: OnceLock<AtomicU64> = OnceLock::new();
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    NEXT_SALT
        .get_or_init(|| AtomicU64::new(seed))
        .fetch_add(1, Ordering::Relaxed)
}

fn imported_complete_address(account: ImportedTestAccount) -> CompleteAddress {
    let expected_address =
        AztecAddress(Fr::from_hex(account.address).expect("valid test account address"));
    let secret_key = Fr::from_hex(account.secret_key).expect("valid test account secret key");
    let partial_address =
        Fr::from_hex(account.partial_address).expect("valid test account partial address");
    let complete =
        complete_address_from_secret_key_and_partial_address(&secret_key, &partial_address)
            .expect("derive complete address");
    assert_eq!(
        complete.address, expected_address,
        "imported fixture address does not match derived complete address for {}",
        account.alias
    );
    complete
}

async fn setup_wallet(account: ImportedTestAccount) -> Option<(TestWallet, AztecAddress)> {
    let url = node_url();
    let node = create_aztec_node_client(&url);
    if let Err(_err) = node.get_node_info().await {
        return None;
    }

    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = match EmbeddedPxe::create(node.clone(), kv).await {
        Ok(pxe) => pxe,
        Err(_err) => {
            return None;
        }
    };

    let secret_key = Fr::from_hex(account.secret_key).expect("valid test account secret key");
    let complete = imported_complete_address(account);

    if let Err(_err) = pxe.key_store().add_account(&secret_key).await {
        return None;
    }
    if let Err(_err) = pxe.address_store().add(&complete).await {
        return None;
    }

    let account_contract = SchnorrAccountContract::new(secret_key);
    let provider =
        SingleAccountProvider::new(complete.clone(), Box::new(account_contract), account.alias);
    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, complete.address))
}

async fn register_contract_on_pxe(
    pxe: &impl Pxe,
    artifact: &ContractArtifact,
    instance: &aztec_rs::types::ContractInstanceWithAddress,
) {
    pxe.register_contract_class(artifact)
        .await
        .expect("register class");
    pxe.register_contract(RegisterContractRequest {
        instance: instance.clone(),
        artifact: Some(artifact.clone()),
    })
    .await
    .expect("register contract");
}

// ---------------------------------------------------------------------------
// Token interaction helpers
// ---------------------------------------------------------------------------

/// Deploy the Token contract. Mirrors upstream:
/// `TokenContract.deploy(wallet, admin, 'TEST', '$TST', 18).send({ from: admin })`
async fn deploy_token(
    wallet: &TestWallet,
    admin: AztecAddress,
) -> (
    AztecAddress,
    ContractArtifact,
    aztec_rs::types::ContractInstanceWithAddress,
) {
    let artifact = load_compiled_token_artifact();
    let deploy = Contract::deploy(
        wallet,
        artifact.clone(),
        vec![
            AbiValue::Field(Fr::from(admin)),
            AbiValue::String("TEST".to_owned()),
            AbiValue::String("$TST".to_owned()),
            AbiValue::Integer(18),
        ],
        None,
    )
    .expect("token deploy builder");

    let deploy_result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: admin,
                ..Default::default()
            },
        )
        .await
        .expect("deploy token");

    let instance = deploy_result.instance;
    let token_address = instance.address;
    (token_address, artifact, instance)
}

/// Send a token contract method and return the [`SendResult`] (with tx hash).
async fn send_token_tx(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    token_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    from: AztecAddress,
) -> SendResult {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|_| panic!("function '{method_name}' not found in artifact"));
    let selector = func.selector.expect("selector");
    let call = FunctionCall {
        to: token_address,
        selector,
        args,
        function_type: func.function_type.clone(),
        is_static: false,
        hide_msg_sender: false,
    };
    wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from,
                ..Default::default()
            },
        )
        .await
        .expect("send tx")
}

async fn expect_token_balance(
    wallet: &TestWallet,
    token_address: AztecAddress,
    artifact: &ContractArtifact,
    owner: AztecAddress,
    expected: u64,
) {
    let func = artifact
        .find_function("balance_of_private")
        .expect("balance_of_private");
    let selector = func.selector.expect("selector");
    let call = FunctionCall {
        to: token_address,
        selector,
        args: vec![AbiValue::Field(Fr::from(owner))],
        function_type: FunctionType::Utility,
        is_static: false,
        hide_msg_sender: false,
    };

    let result = wallet
        .execute_utility(
            call,
            aztec_rs::wallet::ExecuteUtilityOptions {
                scope: owner,
                auth_witnesses: vec![],
            },
        )
        .await
        .expect("execute balance_of_private");

    let balance = result
        .result
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .and_then(|s| Fr::from_hex(s).ok())
        .map_or(0, |f| f.to_usize() as u64);

    assert_eq!(
        balance, expected,
        "expected balance {expected} for {owner}, got {balance}"
    );
}

// ---------------------------------------------------------------------------
// Admin / node helpers
// ---------------------------------------------------------------------------

fn admin_url() -> String {
    std::env::var("AZTEC_ADMIN_URL").unwrap_or_else(|_| {
        // Default: admin API on port 8880, same host as node.
        let node = node_url();
        node.rsplit_once(':').map_or_else(
            || "http://localhost:8880".to_owned(),
            |(host, _)| format!("{host}:8880"),
        )
    })
}

/// Mirrors upstream `aztecNodeAdmin.setConfig({ ... })`.
/// Tries the admin port (default 8880), falls back to the node port.
async fn set_node_config(config: serde_json::Value) {
    for url in [admin_url(), node_url()] {
        let transport = aztec_rpc::RpcTransport::new(url.clone(), Duration::from_secs(5));
        match transport
            .call_void("nodeAdmin_setConfig", serde_json::json!([config]))
            .await
        {
            Ok(()) => {
                return;
            }
            Err(_err) => {}
        }
    }
}

// ===========================================================================
// describe('e2e_pruned_blocks')
// ===========================================================================

/// TS: it('can discover and use notes created in both pruned and available blocks')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn can_discover_and_use_notes_created_in_both_pruned_and_available_blocks() {
    // --- Setup: 3 accounts (admin, sender, recipient), deploy token ---

    let Some((admin_wallet, admin)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let Some((sender_wallet, sender)) = setup_wallet(TEST_ACCOUNT_1).await else {
        return;
    };
    let Some((recipient_wallet, recipient)) = setup_wallet(TEST_ACCOUNT_2).await else {
        return;
    };

    // Register senders cross-PXE for note discovery
    admin_wallet
        .pxe()
        .register_sender(&sender)
        .await
        .expect("admin registers sender");
    admin_wallet
        .pxe()
        .register_sender(&recipient)
        .await
        .expect("admin registers recipient");
    sender_wallet
        .pxe()
        .register_sender(&admin)
        .await
        .expect("sender registers admin");
    sender_wallet
        .pxe()
        .register_sender(&recipient)
        .await
        .expect("sender registers recipient");
    recipient_wallet
        .pxe()
        .register_sender(&admin)
        .await
        .expect("recipient registers admin");
    recipient_wallet
        .pxe()
        .register_sender(&sender)
        .await
        .expect("recipient registers sender");

    let (token_address, artifact, instance) = deploy_token(&admin_wallet, admin).await;

    // Register token contract on sender and recipient PXEs
    register_contract_on_pxe(sender_wallet.pxe(), &artifact, &instance).await;
    register_contract_on_pxe(recipient_wallet.pxe(), &artifact, &instance).await;

    // --- Step 1: Mint first half of MINT_AMOUNT to sender ---
    let first_mint = send_token_tx(
        &admin_wallet,
        &artifact,
        token_address,
        "mint_to_private",
        vec![
            AbiValue::Field(Fr::from(sender)),
            AbiValue::Integer(i128::from(MINT_AMOUNT / 2)),
        ],
        admin,
    )
    .await;

    // --- Step 2: Verify the minted note hash exists and is findable ---
    let first_mint_receipt = admin_wallet
        .node()
        .get_tx_receipt(&first_mint.tx_hash)
        .await
        .expect("get first mint receipt");
    let first_mint_block = first_mint_receipt.block_number.expect("block number");

    let tx_effect = admin_wallet
        .node()
        .get_tx_effect(&first_mint.tx_hash)
        .await
        .expect("get tx effect")
        .expect("tx effect exists");

    // mint_to_private should create just one new note hash
    let note_hashes = tx_effect
        .pointer("/data/noteHashes")
        .and_then(|v| v.as_array())
        .expect("noteHashes in tx effect");
    assert_eq!(
        note_hashes.len(),
        1,
        "mint_to_private should create exactly 1 note hash"
    );

    let minted_note =
        Fr::from_hex(note_hashes[0].as_str().expect("note hash is string")).expect("parse Fr");

    // Historical query for the leaf index at the first mint's block — should succeed
    let leaf_indexes = admin_wallet
        .node()
        .find_leaves_indexes(first_mint_block, NOTE_HASH_TREE, &[minted_note])
        .await
        .expect("find leaf indexes");
    assert!(
        leaf_indexes[0].is_some(),
        "leaf index should exist for the minted note, got {leaf_indexes:?}"
    );

    // --- Steps 3+4: Mine blocks until the first mint block gets pruned ---
    // The upstream TS test uses `setConfig({ minTxsPerBlock: 0 })` and mines a
    // fixed number of blocks. Here we adaptively mine blocks until the historical
    // query for the first mint's block fails, accommodating any checkpoint history.
    set_node_config(serde_json::json!({ "minTxsPerBlock": 0 })).await;
    let mut blocks_mined = 0usize;
    loop {
        // Check if the block is already pruned
        match admin_wallet
            .node()
            .find_leaves_indexes(first_mint_block, NOTE_HASH_TREE, &[minted_note])
            .await
        {
            Err(err) if err.to_string().contains("Unable to find leaf") => {
                break;
            }
            Err(err) => {
                panic!("unexpected error from find_leaves_indexes: {err}");
            }
            Ok(_) => {}
        }

        assert!(
            blocks_mined < MAX_BLOCKS_TO_MINE,
            "gave up after mining {MAX_BLOCKS_TO_MINE} blocks — block {first_mint_block} \
             still not pruned. Is the node configured with a very large \
             worldStateCheckpointHistory?"
        );

        send_token_tx(
            &admin_wallet,
            &artifact,
            token_address,
            "mint_to_private",
            vec![AbiValue::Field(Fr::from(admin)), AbiValue::Integer(1)],
            admin,
        )
        .await;
        blocks_mined += 1;
        if blocks_mined.is_multiple_of(10) {}
    }

    // --- Step 5: Mint second half of MINT_AMOUNT to sender ---
    send_token_tx(
        &admin_wallet,
        &artifact,
        token_address,
        "mint_to_private",
        vec![
            AbiValue::Field(Fr::from(sender)),
            AbiValue::Integer(i128::from(MINT_AMOUNT / 2)),
        ],
        admin,
    )
    .await;

    // --- Step 6: Transfer full MINT_AMOUNT from sender to recipient ---
    // This requires discovering and proving BOTH the old (pruned-block) and new notes.
    send_token_tx(
        &sender_wallet,
        &artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(recipient)),
            AbiValue::Integer(i128::from(MINT_AMOUNT)),
        ],
        sender,
    )
    .await;

    // --- Step 7: Verify recipient balance == MINT_AMOUNT ---
    expect_token_balance(
        &recipient_wallet,
        token_address,
        &artifact,
        recipient,
        MINT_AMOUNT,
    )
    .await;

    // --- Step 8: Verify sender balance == 0 ---
    expect_token_balance(&sender_wallet, token_address, &artifact, sender, 0).await;
}
