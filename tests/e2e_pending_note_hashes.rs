//! Pending note hashes tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_pending_note_hashes_contract.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_pending_note_hashes -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::too_many_lines,
    dead_code,
    unused_imports
)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionSelector, FunctionType};
use aztec_rs::account::{AccountContract, SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::constants::{
    MAX_NOTE_HASHES_PER_CALL, MAX_NOTE_HASHES_PER_TX, MAX_NOTE_HASH_READ_REQUESTS_PER_CALL,
    MAX_NOTE_HASH_READ_REQUESTS_PER_TX,
};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::Pxe;
use aztec_rs::tx::{ExecutionPayload, FunctionCall};
use aztec_rs::types::{
    AztecAddress, CompleteAddress, ContractInstance, ContractInstanceWithAddress, Fr,
};
use aztec_rs::wallet::{BaseWallet, SendOptions, Wallet};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_pending_note_hashes_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/pending_note_hashes_contract_compiled.json");
    ContractArtifact::from_nargo_json(json)
        .expect("parse pending_note_hashes_contract_compiled.json")
}

fn load_schnorr_account_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/schnorr_account_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse schnorr_account_contract_compiled.json")
}

// ---------------------------------------------------------------------------
// Setup helpers
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

fn node_url() -> String {
    std::env::var("AZTEC_NODE_URL").unwrap_or_else(|_| "http://localhost:8080".to_owned())
}

fn serial_guard() -> MutexGuard<'static, ()> {
    static E2E_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    E2E_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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

async fn register_account_for_authwit(
    pxe: &EmbeddedPxe<HttpNodeClient>,
    compiled_artifact: &ContractArtifact,
    account: ImportedTestAccount,
) {
    let secret_key = Fr::from_hex(account.secret_key).expect("valid sk");
    let account_contract = SchnorrAccountContract::new(secret_key);
    let dynamic_artifact = account_contract
        .contract_artifact()
        .await
        .expect("dynamic artifact");
    let complete = imported_complete_address(account);

    let class_id = aztec_rs::hash::compute_contract_class_id_from_artifact(&dynamic_artifact)
        .expect("compute class id");

    pxe.contract_store()
        .add_artifact(&class_id, compiled_artifact)
        .await
        .expect("register compiled account artifact");

    let instance = ContractInstanceWithAddress {
        address: complete.address,
        inner: ContractInstance {
            version: 1,
            salt: Fr::from(0u64),
            deployer: AztecAddress::zero(),
            current_contract_class_id: class_id,
            original_contract_class_id: class_id,
            initialization_hash: Fr::zero(),
            public_keys: complete.public_keys.clone(),
        },
    };
    pxe.contract_store()
        .add_instance(&instance)
        .await
        .expect("register account instance");
}

async fn create_wallet(primary: ImportedTestAccount) -> Option<(TestWallet, AztecAddress)> {
    let url = node_url();
    let node = create_aztec_node_client(&url);
    if let Err(err) = node.get_node_info().await {
        eprintln!("skipping: node not reachable: {err}");
        return None;
    }

    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = match EmbeddedPxe::create(node.clone(), kv).await {
        Ok(pxe) => pxe,
        Err(err) => {
            eprintln!("skipping: failed to create PXE: {err}");
            return None;
        }
    };

    let secret_key = Fr::from_hex(primary.secret_key).expect("valid secret key");
    let complete = imported_complete_address(primary);
    pxe.key_store()
        .add_account(&secret_key)
        .await
        .expect("seed key store");
    pxe.address_store()
        .add(&complete)
        .await
        .expect("seed address store");

    let compiled_account = load_schnorr_account_artifact();
    register_account_for_authwit(&pxe, &compiled_account, primary).await;

    let account_contract = SchnorrAccountContract::new(secret_key);

    let signing_pk = account_contract.signing_public_key();
    let note = aztec_rs::embedded_pxe::stores::note_store::StoredNote {
        contract_address: complete.address,
        owner: complete.address,
        storage_slot: Fr::from(1u64),
        randomness: Fr::zero(),
        note_nonce: Fr::from(1u64),
        note_hash: Fr::from(1u64),
        siloed_nullifier: Fr::from_hex(
            "0xdeadbeef00000000000000000000000000000000000000000000000000000001",
        )
        .expect("unique nullifier"),
        note_data: vec![signing_pk.x, signing_pk.y],
        nullified: false,
        is_pending: false,
        nullification_block_number: None,
        leaf_index: None,
        block_number: None,
        tx_index_in_block: None,
        note_index_in_tx: None,
        scopes: vec![complete.address],
    };
    pxe.note_store()
        .add_note(&note)
        .await
        .expect("seed signing key note");

    let provider =
        SingleAccountProvider::new(complete.clone(), Box::new(account_contract), primary.alias);
    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, complete.address))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_call(
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
) -> FunctionCall {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|_| panic!("function '{method_name}' not found in artifact"));
    let selector = func.selector.expect("selector");
    FunctionCall {
        to: contract_address,
        selector,
        args,
        function_type: func.function_type.clone(),
        is_static: func.is_static,
        hide_msg_sender: false,
    }
}

fn abi_address(addr: AztecAddress) -> AbiValue {
    let mut fields = BTreeMap::new();
    fields.insert("inner".to_owned(), AbiValue::Field(Fr::from(addr)));
    AbiValue::Struct(fields)
}

fn abi_selector(selector: FunctionSelector) -> AbiValue {
    let mut fields = BTreeMap::new();
    fields.insert(
        "inner".to_owned(),
        AbiValue::Integer(u32::from_be_bytes(selector.0).into()),
    );
    AbiValue::Struct(fields)
}

/// Get the function selector for a named function from the artifact.
fn get_selector(artifact: &ContractArtifact, name: &str) -> FunctionSelector {
    artifact
        .find_function(name)
        .unwrap_or_else(|_| panic!("function '{name}' not found"))
        .selector
        .expect("selector")
}

// ---------------------------------------------------------------------------
// Block inspection helpers (mirrors upstream expectNoteHashes/Nullifiers/NoteLogsSquashedExcept)
// ---------------------------------------------------------------------------

/// Get the latest block as JSON from the node.
async fn get_latest_block(node: &HttpNodeClient) -> serde_json::Value {
    let block_num = node.get_block_number().await.expect("get block number");
    node.get_block(block_num)
        .await
        .expect("get block")
        .expect("block should exist")
}

/// Extract all note hashes from all tx effects in a block.
fn extract_note_hashes(block: &serde_json::Value) -> Vec<Fr> {
    let mut result = Vec::new();
    if let Some(effects) = block.pointer("/body/txEffects").and_then(|v| v.as_array()) {
        for effect in effects {
            if let Some(nhs) = effect.get("noteHashes").and_then(|v| v.as_array()) {
                for nh in nhs {
                    if let Some(s) = nh.as_str() {
                        result.push(Fr::from_hex(s).expect("parse note hash"));
                    }
                }
            }
        }
    }
    result
}

/// Extract all nullifiers from all tx effects in a block.
fn extract_nullifiers(block: &serde_json::Value) -> Vec<Fr> {
    let mut result = Vec::new();
    if let Some(effects) = block.pointer("/body/txEffects").and_then(|v| v.as_array()) {
        for effect in effects {
            if let Some(nulls) = effect.get("nullifiers").and_then(|v| v.as_array()) {
                for n in nulls {
                    if let Some(s) = n.as_str() {
                        result.push(Fr::from_hex(s).expect("parse nullifier"));
                    }
                }
            }
        }
    }
    result
}

/// Count all private logs across all tx effects in a block.
fn count_private_logs(block: &serde_json::Value) -> usize {
    let mut count = 0;
    if let Some(effects) = block.pointer("/body/txEffects").and_then(|v| v.as_array()) {
        for effect in effects {
            if let Some(logs) = effect.get("privateLogs").and_then(|v| v.as_array()) {
                count += logs.len();
            }
        }
    }
    count
}

/// Mirrors upstream `expectNoteHashesSquashedExcept(exceptFirstFew)`.
/// Asserts the first `except_first_few` note hashes are non-zero and the rest are zero.
async fn expect_note_hashes_squashed_except(node: &HttpNodeClient, except_first_few: usize) {
    let block = get_latest_block(node).await;
    let note_hashes = extract_note_hashes(&block);

    for (i, nh) in note_hashes.iter().enumerate() {
        if i < except_first_few {
            assert!(
                !nh.is_zero(),
                "note hash at index {i} should be non-zero (expected {except_first_few} non-zero)"
            );
        } else {
            assert!(
                nh.is_zero(),
                "note hash at index {i} should be zero (squashed), got {nh}"
            );
        }
    }
}

/// Mirrors upstream `expectNullifiersSquashedExcept(exceptFirstFew)`.
/// The 0th nullifier is always non-zero (txHash). The next `except_first_few` should also
/// be non-zero, and all remaining should be zero.
async fn expect_nullifiers_squashed_except(node: &HttpNodeClient, except_first_few: usize) {
    let block = get_latest_block(node).await;
    let nullifiers = extract_nullifiers(&block);

    for (i, n) in nullifiers.iter().enumerate() {
        if i < except_first_few + 1 {
            assert!(
                !n.is_zero(),
                "nullifier at index {i} should be non-zero (expected {except_first_few}+1 non-zero)"
            );
        } else {
            assert!(
                n.is_zero(),
                "nullifier at index {i} should be zero (squashed), got {n}"
            );
        }
    }
}

/// Mirrors upstream `expectNoteLogsSquashedExcept(exceptFirstFew)`.
/// Asserts the total number of private logs equals `except_first_few`.
async fn expect_note_logs_squashed_except(node: &HttpNodeClient, except_first_few: usize) {
    let block = get_latest_block(node).await;
    let log_count = count_private_logs(&block);
    assert_eq!(
        log_count, except_first_few,
        "expected {except_first_few} private logs, got {log_count}"
    );
}

// ---------------------------------------------------------------------------
// Deploy helper
// ---------------------------------------------------------------------------

async fn deploy_contract(
    wallet: &TestWallet,
    owner: AztecAddress,
) -> (AztecAddress, ContractArtifact) {
    let artifact = load_pending_note_hashes_artifact();
    eprintln!("deploying PendingNoteHashesContract...");
    let deploy = Contract::deploy(wallet, artifact.clone(), vec![], None).expect("deploy setup");
    let deploy_result = deploy
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
        .expect("deploy PendingNoteHashesContract");
    let contract_address = deploy_result.instance.address;
    eprintln!("PendingNoteHashesContract deployed at {contract_address}");
    (contract_address, artifact)
}

/// Send a transaction calling a single method on the contract.
async fn send_method(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    owner: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
) {
    let call = build_call(artifact, contract_address, method_name, args);
    wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: owner,
                ..Default::default()
            },
        )
        .await
        .unwrap_or_else(|e| panic!("{method_name} failed: {e}"));
}

// ---------------------------------------------------------------------------
// Tests: e2e_pending_note_hashes_contract
// ---------------------------------------------------------------------------

/// TS: Aztec.nr function can "get" notes it just "inserted"
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn function_can_get_notes_it_just_inserted() {
    let _guard = serial_guard();
    let Some((wallet, owner)) = create_wallet(TEST_ACCOUNT_0).await else {
        return;
    };

    let mint_amount = 65u64;
    let (contract_address, artifact) = deploy_contract(&wallet, owner).await;

    let sender = owner;
    eprintln!("calling test_insert_then_get_then_nullify_flat...");
    send_method(
        &wallet,
        &artifact,
        contract_address,
        owner,
        "test_insert_then_get_then_nullify_flat",
        vec![
            AbiValue::Field(Fr::from(mint_amount)),
            abi_address(owner),
            abi_address(sender),
        ],
    )
    .await;
    eprintln!("test_insert_then_get_then_nullify_flat succeeded");
}

/// TS: Squash! Aztec.nr function can "create" and "nullify" note in the same TX
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn squash_create_and_nullify_in_same_tx() {
    let _guard = serial_guard();
    let Some((wallet, owner)) = create_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let node = wallet.node().clone();

    let mint_amount = 65u64;
    let (contract_address, artifact) = deploy_contract(&wallet, owner).await;

    let sender = owner;
    let insert_selector = get_selector(&artifact, "insert_note");
    let nullify_selector = get_selector(&artifact, "get_then_nullify_note");

    eprintln!("calling test_insert_then_get_then_nullify_all_in_nested_calls...");
    send_method(
        &wallet,
        &artifact,
        contract_address,
        owner,
        "test_insert_then_get_then_nullify_all_in_nested_calls",
        vec![
            AbiValue::Field(Fr::from(mint_amount)),
            abi_address(owner),
            abi_address(sender),
            abi_selector(insert_selector),
            abi_selector(nullify_selector),
        ],
    )
    .await;

    eprintln!("calling get_note_zero_balance...");
    send_method(
        &wallet,
        &artifact,
        contract_address,
        owner,
        "get_note_zero_balance",
        vec![abi_address(owner)],
    )
    .await;

    expect_note_hashes_squashed_except(&node, 0).await;
    expect_nullifiers_squashed_except(&node, 0).await;
    expect_note_logs_squashed_except(&node, 0).await;
    eprintln!("squash_create_and_nullify_in_same_tx passed");
}

/// TS: Squash! ... with 2 note logs
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn squash_create_and_nullify_in_same_tx_with_2_note_logs() {
    let _guard = serial_guard();
    let Some((wallet, owner)) = create_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let node = wallet.node().clone();

    let mint_amount = 65u64;
    let (contract_address, artifact) = deploy_contract(&wallet, owner).await;

    let sender = owner;
    let insert_selector = get_selector(&artifact, "insert_note_extra_emit");
    let nullify_selector = get_selector(&artifact, "get_then_nullify_note");

    eprintln!("calling test_insert_then_get_then_nullify_all_in_nested_calls (extra emit)...");
    send_method(
        &wallet,
        &artifact,
        contract_address,
        owner,
        "test_insert_then_get_then_nullify_all_in_nested_calls",
        vec![
            AbiValue::Field(Fr::from(mint_amount)),
            abi_address(owner),
            abi_address(sender),
            abi_selector(insert_selector),
            abi_selector(nullify_selector),
        ],
    )
    .await;

    expect_note_hashes_squashed_except(&node, 0).await;
    expect_nullifiers_squashed_except(&node, 0).await;
    expect_note_logs_squashed_except(&node, 0).await;
    eprintln!("squash_create_and_nullify_in_same_tx_with_2_note_logs passed");
}

/// TS: Squash! ... create 2 notes and nullify both in the same TX
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn squash_create_2_notes_and_nullify_both_in_same_tx() {
    let _guard = serial_guard();
    let Some((wallet, owner)) = create_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let node = wallet.node().clone();

    let mint_amount = 65u64;
    let (contract_address, artifact) = deploy_contract(&wallet, owner).await;

    let sender = owner;
    let insert_selector = get_selector(&artifact, "insert_note");
    let nullify_selector = get_selector(&artifact, "get_then_nullify_note");

    eprintln!("calling test_insert2_then_get2_then_nullify2_all_in_nested_calls...");
    send_method(
        &wallet,
        &artifact,
        contract_address,
        owner,
        "test_insert2_then_get2_then_nullify2_all_in_nested_calls",
        vec![
            AbiValue::Field(Fr::from(mint_amount)),
            abi_address(owner),
            abi_address(sender),
            abi_selector(insert_selector),
            abi_selector(nullify_selector),
        ],
    )
    .await;

    expect_note_hashes_squashed_except(&node, 0).await;
    expect_nullifiers_squashed_except(&node, 0).await;
    expect_note_logs_squashed_except(&node, 0).await;
    eprintln!("squash_create_2_notes_and_nullify_both_in_same_tx passed");
}

/// TS: Squash! ... create 2 notes and nullify 1 in the same TX
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn squash_create_2_notes_and_nullify_1_in_same_tx() {
    let _guard = serial_guard();
    let Some((wallet, owner)) = create_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let node = wallet.node().clone();

    let mint_amount = 65u64;
    let (contract_address, artifact) = deploy_contract(&wallet, owner).await;

    let sender = owner;
    let insert_selector = get_selector(&artifact, "insert_note");
    let nullify_selector = get_selector(&artifact, "get_then_nullify_note");

    eprintln!("calling test_insert2_then_get2_then_nullify1_all_in_nested_calls...");
    send_method(
        &wallet,
        &artifact,
        contract_address,
        owner,
        "test_insert2_then_get2_then_nullify1_all_in_nested_calls",
        vec![
            AbiValue::Field(Fr::from(mint_amount)),
            abi_address(owner),
            abi_address(sender),
            abi_selector(insert_selector),
            abi_selector(nullify_selector),
        ],
    )
    .await;

    expect_note_hashes_squashed_except(&node, 1).await;
    expect_nullifiers_squashed_except(&node, 0).await;
    expect_note_logs_squashed_except(&node, 1).await;
    eprintln!("squash_create_2_notes_and_nullify_1_in_same_tx passed");
}

/// TS: Squash! ... create 2 notes with the same note hash and nullify 1 in the same TX
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn squash_create_2_notes_same_hash_nullify_1_in_same_tx() {
    let _guard = serial_guard();
    let Some((wallet, owner)) = create_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let node = wallet.node().clone();

    let mint_amount = 65u64;
    let (contract_address, artifact) = deploy_contract(&wallet, owner).await;

    let sender = owner;
    let insert_selector = get_selector(&artifact, "insert_note_static_randomness");
    let nullify_selector = get_selector(&artifact, "get_then_nullify_note");

    eprintln!("calling test_insert2_then_get2_then_nullify1 (static randomness)...");
    send_method(
        &wallet,
        &artifact,
        contract_address,
        owner,
        "test_insert2_then_get2_then_nullify1_all_in_nested_calls",
        vec![
            AbiValue::Field(Fr::from(mint_amount)),
            abi_address(owner),
            abi_address(sender),
            abi_selector(insert_selector),
            abi_selector(nullify_selector),
        ],
    )
    .await;

    expect_note_hashes_squashed_except(&node, 1).await;
    expect_nullifiers_squashed_except(&node, 0).await;
    expect_note_logs_squashed_except(&node, 1).await;
    eprintln!("squash_create_2_notes_same_hash_nullify_1_in_same_tx passed");
}

/// TS: Squash! ... nullify a pending note and a persistent in the same TX
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn squash_nullify_pending_and_persistent_in_same_tx() {
    let _guard = serial_guard();
    let Some((wallet, owner)) = create_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let node = wallet.node().clone();

    let mint_amount = 65u64;
    let (contract_address, artifact) = deploy_contract(&wallet, owner).await;

    let sender = owner;

    // TX1: create a persistent note
    eprintln!("TX1: inserting persistent note...");
    send_method(
        &wallet,
        &artifact,
        contract_address,
        owner,
        "insert_note",
        vec![
            AbiValue::Field(Fr::from(mint_amount)),
            abi_address(owner),
            abi_address(sender),
        ],
    )
    .await;

    // Verify TX1 created 1 persistent note
    expect_note_hashes_squashed_except(&node, 1).await;
    expect_nullifiers_squashed_except(&node, 0).await;
    expect_note_logs_squashed_except(&node, 1).await;

    // TX2: create another note, and nullify it AND the persistent note
    let insert_selector = get_selector(&artifact, "insert_note");
    let nullify_selector = get_selector(&artifact, "get_then_nullify_note");

    eprintln!("TX2: insert 1, get 2, nullify 2...");
    send_method(
        &wallet,
        &artifact,
        contract_address,
        owner,
        "test_insert1_then_get2_then_nullify2_all_in_nested_calls",
        vec![
            AbiValue::Field(Fr::from(mint_amount)),
            abi_address(owner),
            abi_address(sender),
            abi_selector(insert_selector),
            abi_selector(nullify_selector),
        ],
    )
    .await;

    // TX3: verify zero balance
    eprintln!("TX3: verifying zero balance...");
    send_method(
        &wallet,
        &artifact,
        contract_address,
        owner,
        "get_note_zero_balance",
        vec![abi_address(owner)],
    )
    .await;

    // Second TX: 1 note created but squashed, 1 nullifier for persistent note remains
    expect_note_hashes_squashed_except(&node, 0).await;
    expect_nullifiers_squashed_except(&node, 1).await;
    expect_note_logs_squashed_except(&node, 0).await;
    eprintln!("squash_nullify_pending_and_persistent_in_same_tx passed");
}

/// TS: `get_notes` function filters a nullified note created in a previous transaction
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn get_notes_filters_nullified_note_from_previous_tx() {
    let _guard = serial_guard();
    let Some((wallet, owner)) = create_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let node = wallet.node().clone();

    let mint_amount = 65u64;
    let (contract_address, artifact) = deploy_contract(&wallet, owner).await;

    let sender = owner;

    // TX1: create a note
    eprintln!("TX1: inserting note...");
    send_method(
        &wallet,
        &artifact,
        contract_address,
        owner,
        "insert_note",
        vec![
            AbiValue::Field(Fr::from(mint_amount)),
            abi_address(owner),
            abi_address(sender),
        ],
    )
    .await;

    // There is a single new note hash
    expect_note_hashes_squashed_except(&node, 1).await;
    expect_note_logs_squashed_except(&node, 1).await;

    // TX2: use dummy as insert (no-op), then get and nullify the persistent note
    let dummy_selector = get_selector(&artifact, "dummy");
    let nullify_selector = get_selector(&artifact, "get_then_nullify_note");

    eprintln!("TX2: nullify persistent note via nested calls...");
    send_method(
        &wallet,
        &artifact,
        contract_address,
        owner,
        "test_insert_then_get_then_nullify_all_in_nested_calls",
        vec![
            AbiValue::Field(Fr::from(mint_amount)),
            abi_address(owner),
            abi_address(sender),
            abi_selector(dummy_selector),
            abi_selector(nullify_selector),
        ],
    )
    .await;

    // There is a single new nullifier (for the persistent note)
    expect_nullifiers_squashed_except(&node, 1).await;
    eprintln!("get_notes_filters_nullified_note_from_previous_tx passed");
}

/// TS: Should handle overflowing the kernel data structures in nested calls
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn handle_overflowing_kernel_data_in_nested_calls() {
    let _guard = serial_guard();
    let Some((wallet, owner)) = create_wallet(TEST_ACCOUNT_0).await else {
        return;
    };

    let (contract_address, artifact) = deploy_contract(&wallet, owner).await;

    let notes_per_iteration = std::cmp::min(
        MAX_NOTE_HASHES_PER_CALL,
        MAX_NOTE_HASH_READ_REQUESTS_PER_CALL,
    );
    let min_to_need_reset =
        std::cmp::min(MAX_NOTE_HASHES_PER_TX, MAX_NOTE_HASH_READ_REQUESTS_PER_TX) + 1;
    let recursions = min_to_need_reset.div_ceil(notes_per_iteration);

    eprintln!(
        "calling test_recursively_create_notes with {recursions} recursions \
         (notes_per_iter={notes_per_iteration}, min_to_need_reset={min_to_need_reset})..."
    );
    send_method(
        &wallet,
        &artifact,
        contract_address,
        owner,
        "test_recursively_create_notes",
        vec![abi_address(owner), AbiValue::Integer(recursions as i128)],
    )
    .await;
    eprintln!("handle_overflowing_kernel_data_in_nested_calls passed");
}
