//! Note getter tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_note_getter.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_note_getter -- --ignored --nocapture
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

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionType};
use aztec_rs::account::{AccountContract, SchnorrAccountContract, SingleAccountProvider};
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
use aztec_rs::wallet::{BaseWallet, ExecuteUtilityOptions, SendOptions, SimulateOptions, Wallet};

use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_note_getter_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/note_getter_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse note_getter_contract_compiled.json")
}

fn load_test_contract_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/test_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse test_contract_compiled.json")
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

/// Parse a utility execution result as a single Fr.
fn utility_result_single(result: &serde_json::Value) -> Fr {
    let arr = result.as_array().expect("utility result should be array");
    let s = arr[0].as_str().expect("field element string");
    Fr::from_hex(s).expect("parse Fr")
}

/// Parse a utility execution result as a Vec<Fr> of the given length.
fn utility_result_array(result: &serde_json::Value, count: usize) -> Vec<Fr> {
    let arr = result.as_array().expect("utility result should be array");
    (0..count)
        .map(|i| {
            let s = arr[i].as_str().expect("field element string");
            Fr::from_hex(s).expect("parse Fr")
        })
        .collect()
}

/// Parse a `BoundedVec<Field, N>` from a utility execution result.
/// Layout: N storage fields followed by 1 len field.
fn utility_result_bounded_vec(result: &serde_json::Value, max_len: usize) -> Vec<Fr> {
    let arr = result.as_array().expect("utility result should be array");
    let len_str = arr[max_len].as_str().expect("len field");
    let len = Fr::from_hex(len_str).expect("parse len").to_usize();
    (0..len)
        .map(|i| {
            let s = arr[i].as_str().expect("field element string");
            Fr::from_hex(s).expect("parse Fr")
        })
        .collect()
}

/// Extract a single return value from a private function simulation result.
fn simulate_result_single(result: &serde_json::Value) -> Fr {
    let s = result
        .pointer("/returnValues/0")
        .and_then(|v| v.as_str())
        .expect("returnValues/0 should be a string");
    Fr::from_hex(s).expect("parse Fr")
}

/// Extract multiple return values from a private function simulation result.
fn simulate_result_array(result: &serde_json::Value, count: usize) -> Vec<Fr> {
    (0..count)
        .map(|i| {
            let ptr = format!("/returnValues/{i}");
            let s = result
                .pointer(&ptr)
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("returnValues/{i} should be a string"));
            Fr::from_hex(s).expect("parse Fr")
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Comparator constants (mirrors upstream Comparator enum)
// ---------------------------------------------------------------------------

const COMPARATOR_EQ: i128 = 1;
const COMPARATOR_NEQ: i128 = 2;
const COMPARATOR_LT: i128 = 3;
const COMPARATOR_LTE: i128 = 4;
const COMPARATOR_GT: i128 = 5;
const COMPARATOR_GTE: i128 = 6;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct ComparatorState {
    wallet: TestWallet,
    default_account: AztecAddress,
    contract_address: AztecAddress,
    artifact: ContractArtifact,
}

static COMPARATOR_STATE: OnceCell<Option<ComparatorState>> = OnceCell::const_new();

async fn get_comparator_state() -> Option<&'static ComparatorState> {
    COMPARATOR_STATE
        .get_or_init(|| async { init_comparator_state().await })
        .await
        .as_ref()
}

async fn init_comparator_state() -> Option<ComparatorState> {
    let (wallet, default_account) = create_wallet(TEST_ACCOUNT_0).await?;

    let artifact = load_note_getter_artifact();
    let deploy = Contract::deploy(&wallet, artifact.clone(), vec![], None).expect("deploy setup");
    let deploy_result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: default_account,
                ..Default::default()
            },
        )
        .await
        .expect("deploy NoteGetterContract");
    let contract_address = deploy_result.instance.address;

    Some(ComparatorState {
        wallet,
        default_account,
        contract_address,
        artifact,
    })
}

struct StatusFilterState {
    wallet: TestWallet,
    default_account: AztecAddress,
    contract_address: AztecAddress,
    artifact: ContractArtifact,
}

static STATUS_FILTER_STATE: OnceCell<Option<StatusFilterState>> = OnceCell::const_new();

async fn get_status_filter_state() -> Option<&'static StatusFilterState> {
    STATUS_FILTER_STATE
        .get_or_init(|| async { init_status_filter_state().await })
        .await
        .as_ref()
}

async fn init_status_filter_state() -> Option<StatusFilterState> {
    let (wallet, default_account) = create_wallet(TEST_ACCOUNT_0).await?;

    let artifact = load_test_contract_artifact();
    let deploy = Contract::deploy(&wallet, artifact.clone(), vec![], None).expect("deploy setup");
    let deploy_result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: default_account,
                ..Default::default()
            },
        )
        .await
        .expect("deploy TestContract");
    let contract_address = deploy_result.instance.address;

    Some(StatusFilterState {
        wallet,
        default_account,
        contract_address,
        artifact,
    })
}

// ---------------------------------------------------------------------------
// Status filter helpers
// ---------------------------------------------------------------------------

const VALUE: i128 = 5;
const MAKE_TX_HYBRID: bool = false;

/// Send `call_create_note(value, owner, storage_slot, make_tx_hybrid)`.
async fn send_create_note(s: &StatusFilterState, value: i128, storage_slot: u64) {
    let call = build_call(
        &s.artifact,
        s.contract_address,
        "call_create_note",
        vec![
            AbiValue::Integer(value),
            abi_address(s.default_account),
            AbiValue::Field(Fr::from(storage_slot)),
            AbiValue::Boolean(MAKE_TX_HYBRID),
        ],
    );
    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.default_account,
                ..Default::default()
            },
        )
        .await
        .expect("call_create_note should succeed");
}

/// Send `call_destroy_note(owner, storage_slot)`.
async fn send_destroy_note(s: &StatusFilterState, storage_slot: u64) {
    let call = build_call(
        &s.artifact,
        s.contract_address,
        "call_destroy_note",
        vec![
            abi_address(s.default_account),
            AbiValue::Field(Fr::from(storage_slot)),
        ],
    );
    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.default_account,
                ..Default::default()
            },
        )
        .await
        .expect("call_destroy_note should succeed");
}

/// Assert that both `call_view_notes` (utility) and `call_get_notes` (private simulate)
/// return the expected value at the given storage slot.
async fn assert_note_is_returned(
    s: &StatusFilterState,
    storage_slot: u64,
    expected_value: i128,
    active_or_nullified: bool,
) {
    // call_view_notes — utility execution
    let view_call = build_call(
        &s.artifact,
        s.contract_address,
        "call_view_notes",
        vec![
            abi_address(s.default_account),
            AbiValue::Field(Fr::from(storage_slot)),
            AbiValue::Boolean(active_or_nullified),
        ],
    );
    let view_result = s
        .wallet
        .execute_utility(
            view_call,
            ExecuteUtilityOptions {
                scope: s.default_account,
                ..Default::default()
            },
        )
        .await
        .expect("call_view_notes should succeed");
    let view_value = utility_result_single(&view_result.result);

    // call_get_notes — private simulate
    let get_call = build_call(
        &s.artifact,
        s.contract_address,
        "call_get_notes",
        vec![
            abi_address(s.default_account),
            AbiValue::Field(Fr::from(storage_slot)),
            AbiValue::Boolean(active_or_nullified),
        ],
    );
    let get_result = s
        .wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![get_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.default_account,
                ..Default::default()
            },
        )
        .await
        .expect("call_get_notes should succeed");
    let get_value = simulate_result_single(&get_result.return_values);

    let expected = Fr::from(expected_value as u64);
    assert_eq!(
        view_value, expected,
        "call_view_notes returned {view_value}, expected {expected}"
    );
    assert_eq!(
        get_value, expected,
        "call_get_notes returned {get_value}, expected {expected}"
    );
    assert_eq!(
        view_value, get_value,
        "call_view_notes and call_get_notes should return the same value"
    );
}

/// Assert that both `call_view_notes` and `call_get_notes` fail with a
/// `BoundedVec` assertion error (no notes to return).
async fn assert_no_return_value(
    s: &StatusFilterState,
    storage_slot: u64,
    active_or_nullified: bool,
) {
    // call_view_notes should fail (no active notes to return)
    let view_call = build_call(
        &s.artifact,
        s.contract_address,
        "call_view_notes",
        vec![
            abi_address(s.default_account),
            AbiValue::Field(Fr::from(storage_slot)),
            AbiValue::Boolean(active_or_nullified),
        ],
    );
    let view_err = s
        .wallet
        .execute_utility(
            view_call,
            ExecuteUtilityOptions {
                scope: s.default_account,
                ..Default::default()
            },
        )
        .await
        .expect_err("call_view_notes should fail with no notes");
    let view_err_str = view_err.to_string();
    assert!(
        view_err_str.contains("BoundedVec")
            || view_err_str.contains("Failed to solve brillig")
            || view_err_str.contains("Assertion failed"),
        "call_view_notes error should indicate no notes, got: {view_err}"
    );

    // call_get_notes should fail
    let get_call = build_call(
        &s.artifact,
        s.contract_address,
        "call_get_notes",
        vec![
            abi_address(s.default_account),
            AbiValue::Field(Fr::from(storage_slot)),
            AbiValue::Boolean(active_or_nullified),
        ],
    );
    let get_err = s
        .wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![get_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.default_account,
                ..Default::default()
            },
        )
        .await
        .expect_err("call_get_notes should fail with no notes");
    let get_err_str = get_err.to_string();
    assert!(
        get_err_str.contains("BoundedVec")
            || get_err_str.contains("Failed to solve brillig")
            || get_err_str.contains("Assertion failed")
            || get_err_str.contains("Cannot satisfy constraint")
            || get_err_str.contains("reverted"),
        "call_get_notes error should indicate no notes, got: {get_err}"
    );
}

// ---------------------------------------------------------------------------
// Tests: comparators
// ---------------------------------------------------------------------------

/// TS: comparators > inserts notes from 0-9, then makes multiple queries
/// specifying the total suite of comparators
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn inserts_notes_then_queries_with_all_comparators() {
    let _guard = serial_guard();
    let Some(s) = get_comparator_state().await else {
        return;
    };

    // Insert notes with values 0-9
    for i in 0u64..10 {
        let call = build_call(
            &s.artifact,
            s.contract_address,
            "insert_note",
            vec![AbiValue::Field(Fr::from(i))],
        );
        s.wallet
            .send_tx(
                ExecutionPayload {
                    calls: vec![call],
                    ..Default::default()
                },
                SendOptions {
                    from: s.default_account,
                    ..Default::default()
                },
            )
            .await
            .unwrap_or_else(|e| panic!("insert_note({i}) failed: {e}"));
    }

    // Insert a duplicate note with value 5
    let call = build_call(
        &s.artifact,
        s.contract_address,
        "insert_note",
        vec![AbiValue::Field(Fr::from(5u64))],
    );
    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.default_account,
                ..Default::default()
            },
        )
        .await
        .expect("insert_note(5) duplicate failed");

    // Query with each comparator

    let query = |comparator: i128| {
        build_call(
            &s.artifact,
            s.contract_address,
            "read_note_values",
            vec![
                abi_address(s.default_account),
                AbiValue::Integer(comparator),
                AbiValue::Field(Fr::from(5u64)),
            ],
        )
    };

    let exec_query = |call: FunctionCall| async {
        s.wallet
            .execute_utility(
                call,
                ExecuteUtilityOptions {
                    scope: s.default_account,
                    ..Default::default()
                },
            )
            .await
            .expect("read_note_values should succeed")
    };

    let result_eq = exec_query(query(COMPARATOR_EQ)).await;
    let result_neq = exec_query(query(COMPARATOR_NEQ)).await;
    let result_lt = exec_query(query(COMPARATOR_LT)).await;
    let result_gt = exec_query(query(COMPARATOR_GT)).await;
    let result_lte = exec_query(query(COMPARATOR_LTE)).await;
    let result_gte = exec_query(query(COMPARATOR_GTE)).await;

    // Parse BoundedVec<Field, 10> results
    let mut eq_vals: Vec<u64> = utility_result_bounded_vec(&result_eq.result, 10)
        .iter()
        .map(Fr::to_usize)
        .map(|v| v as u64)
        .collect();
    let mut neq_vals: Vec<u64> = utility_result_bounded_vec(&result_neq.result, 10)
        .iter()
        .map(Fr::to_usize)
        .map(|v| v as u64)
        .collect();
    let mut lt_vals: Vec<u64> = utility_result_bounded_vec(&result_lt.result, 10)
        .iter()
        .map(Fr::to_usize)
        .map(|v| v as u64)
        .collect();
    let mut gt_vals: Vec<u64> = utility_result_bounded_vec(&result_gt.result, 10)
        .iter()
        .map(Fr::to_usize)
        .map(|v| v as u64)
        .collect();
    let mut lte_vals: Vec<u64> = utility_result_bounded_vec(&result_lte.result, 10)
        .iter()
        .map(Fr::to_usize)
        .map(|v| v as u64)
        .collect();
    let mut gte_vals: Vec<u64> = utility_result_bounded_vec(&result_gte.result, 10)
        .iter()
        .map(Fr::to_usize)
        .map(|v| v as u64)
        .collect();

    eq_vals.sort_unstable();
    neq_vals.sort_unstable();
    lt_vals.sort_unstable();
    gt_vals.sort_unstable();
    lte_vals.sort_unstable();
    gte_vals.sort_unstable();

    assert_eq!(eq_vals, vec![5, 5], "EQ 5");
    assert_eq!(neq_vals, vec![0, 1, 2, 3, 4, 6, 7, 8, 9], "NEQ 5");
    assert_eq!(lt_vals, vec![0, 1, 2, 3, 4], "LT 5");
    assert_eq!(gt_vals, vec![6, 7, 8, 9], "GT 5");
    assert_eq!(lte_vals, vec![0, 1, 2, 3, 4, 5, 5], "LTE 5");
    assert_eq!(gte_vals, vec![5, 5, 6, 7, 8, 9], "GTE 5");
}

// ---------------------------------------------------------------------------
// Tests: status filter — active note only
// ---------------------------------------------------------------------------

/// TS: active note only > returns active notes
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn active_status_returns_active_notes() {
    let _guard = serial_guard();
    let Some(s) = get_status_filter_state().await else {
        return;
    };

    let storage_slot = 1001;
    let active_or_nullified = false;
    send_create_note(s, VALUE, storage_slot).await;
    assert_note_is_returned(s, storage_slot, VALUE, active_or_nullified).await;
}

/// TS: active note only > does not return nullified notes
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn active_status_does_not_return_nullified_notes() {
    let _guard = serial_guard();
    let Some(s) = get_status_filter_state().await else {
        return;
    };

    let storage_slot = 1002;
    let active_or_nullified = false;
    send_create_note(s, VALUE, storage_slot).await;
    send_destroy_note(s, storage_slot).await;
    assert_no_return_value(s, storage_slot, active_or_nullified).await;
}

// ---------------------------------------------------------------------------
// Tests: status filter — active and nullified notes
// ---------------------------------------------------------------------------

/// TS: active and nullified notes > returns active notes
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn active_and_nullified_returns_active_notes() {
    let _guard = serial_guard();
    let Some(s) = get_status_filter_state().await else {
        return;
    };

    let storage_slot = 1003;
    let active_or_nullified = true;
    send_create_note(s, VALUE, storage_slot).await;
    assert_note_is_returned(s, storage_slot, VALUE, active_or_nullified).await;
}

/// TS: active and nullified notes > returns nullified notes
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn active_and_nullified_returns_nullified_notes() {
    let _guard = serial_guard();
    let Some(s) = get_status_filter_state().await else {
        return;
    };

    let storage_slot = 1004;
    let active_or_nullified = true;
    send_create_note(s, VALUE, storage_slot).await;
    send_destroy_note(s, storage_slot).await;
    assert_note_is_returned(s, storage_slot, VALUE, active_or_nullified).await;
}

/// TS: active and nullified notes > returns both active and nullified notes
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn active_and_nullified_returns_both() {
    let _guard = serial_guard();
    let Some(s) = get_status_filter_state().await else {
        return;
    };

    let storage_slot = 1005;
    let active_or_nullified = true;

    // Create two notes with different values in the same storage slot
    send_create_note(s, VALUE, storage_slot).await;
    send_create_note(s, VALUE + 1, storage_slot).await;

    // Destroy one note
    send_destroy_note(s, storage_slot).await;

    // Fetch multiple notes — both the active and the nullified one
    // call_view_notes_many — utility
    let view_call = build_call(
        &s.artifact,
        s.contract_address,
        "call_view_notes_many",
        vec![
            abi_address(s.default_account),
            AbiValue::Field(Fr::from(storage_slot)),
            AbiValue::Boolean(active_or_nullified),
        ],
    );
    let view_result = s
        .wallet
        .execute_utility(
            view_call,
            ExecuteUtilityOptions {
                scope: s.default_account,
                ..Default::default()
            },
        )
        .await
        .expect("call_view_notes_many should succeed");
    let mut view_values: Vec<u64> = utility_result_array(&view_result.result, 2)
        .iter()
        .map(Fr::to_usize)
        .map(|v| v as u64)
        .collect();

    // call_get_notes_many — private simulate
    let get_call = build_call(
        &s.artifact,
        s.contract_address,
        "call_get_notes_many",
        vec![
            abi_address(s.default_account),
            AbiValue::Field(Fr::from(storage_slot)),
            AbiValue::Boolean(active_or_nullified),
        ],
    );
    let get_result = s
        .wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![get_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.default_account,
                ..Default::default()
            },
        )
        .await
        .expect("call_get_notes_many should succeed");
    let mut get_values: Vec<u64> = simulate_result_array(&get_result.return_values, 2)
        .iter()
        .map(Fr::to_usize)
        .map(|v| v as u64)
        .collect();

    view_values.sort_unstable();
    get_values.sort_unstable();

    // Both methods should return the same result
    assert_eq!(
        view_values, get_values,
        "call_view_notes_many and call_get_notes_many should return the same values"
    );

    // Should contain both VALUE and VALUE+1
    assert_eq!(
        view_values,
        vec![VALUE as u64, (VALUE + 1) as u64],
        "should return both active and nullified notes"
    );
}
