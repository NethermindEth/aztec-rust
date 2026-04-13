//! Ordering tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_ordering.test.ts`.
//!
//! Tests proper sequencing of enqueued public calls, state updates, and logs
//! via parent/child contracts.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_ordering -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
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

use aztec_rs::abi::{encode_arguments, AbiValue, ContractArtifact, FunctionSelector};
use aztec_rs::account::{SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::fee::GasSettings;
use aztec_rs::hash::compute_calldata_hash;
use aztec_rs::node::{
    create_aztec_node_client, wait_for_tx, AztecNode, HttpNodeClient, PublicLogFilter, WaitOpts,
};
use aztec_rs::pxe::Pxe;
use aztec_rs::tx::{ExecutionPayload, FunctionCall, HashedValues, TxHash};
use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
use aztec_rs::wallet::{AccountProvider, BaseWallet, SendOptions, Wallet};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_parent_contract_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/parent_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse parent_contract_compiled.json")
}

fn load_child_contract_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/child_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse child_contract_compiled.json")
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

// ---------------------------------------------------------------------------
// Contract interaction helpers
// ---------------------------------------------------------------------------

fn abi_address(address: AztecAddress) -> AbiValue {
    let mut fields = BTreeMap::new();
    fields.insert("inner".to_owned(), AbiValue::Field(Fr::from(address)));
    AbiValue::Struct(fields)
}

fn abi_selector(selector: FunctionSelector) -> AbiValue {
    let mut fields = BTreeMap::new();
    fields.insert("inner".to_owned(), AbiValue::Field(selector.to_field()));
    AbiValue::Struct(fields)
}

async fn deploy_contract(
    wallet: &TestWallet,
    artifact: ContractArtifact,
    from: AztecAddress,
) -> AztecAddress {
    let deploy = Contract::deploy(wallet, artifact, vec![], None).expect("deploy builder");
    let result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from,
                ..Default::default()
            },
        )
        .await
        .expect("deploy contract");
    result.instance.address
}

async fn get_child_stored_value(node: &HttpNodeClient, child_address: AztecAddress) -> Fr {
    node.get_public_storage_at(0, &child_address, &Fr::from(1u64))
        .await
        .expect("read Child.current_value from public storage")
}

async fn expect_logs_from_last_block_to_be(node: &HttpNodeClient, expected_logs: &[Fr]) {
    let from_block = node
        .get_block_number()
        .await
        .expect("get latest block number");
    let public_logs = node
        .get_public_logs(PublicLogFilter {
            from_block: Some(from_block),
            to_block: Some(from_block + 1),
            ..Default::default()
        })
        .await
        .expect("get public logs");

    let actual_logs = public_logs
        .logs
        .into_iter()
        .map(|log| {
            *log.data
                .first()
                .expect("expected at least one field in emitted public log")
        })
        .collect::<Vec<_>>();

    assert_eq!(actual_logs, expected_logs);
}

fn build_call(
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: &[AbiValue],
) -> FunctionCall {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|_| panic!("function '{method_name}' not found in artifact"));
    let encoded_args = encode_arguments(func, args)
        .unwrap_or_else(|e| panic!("encode arguments for '{method_name}': {e}"));
    FunctionCall {
        to: contract_address,
        selector: func.selector.expect("selector"),
        args: encoded_args.into_iter().map(AbiValue::Field).collect(),
        function_type: func.function_type.clone(),
        is_static: func.is_static,
        hide_msg_sender: false,
    }
}

struct TestCase {
    wallet: TestWallet,
    default_account_address: AztecAddress,
    parent_address: AztecAddress,
    parent_artifact: ContractArtifact,
    child_address: AztecAddress,
    child_artifact: ContractArtifact,
    child_pub_set_value_selector: FunctionSelector,
}

async fn setup_test_case() -> Option<TestCase> {
    let (wallet, default_account_address) = setup_wallet(TEST_ACCOUNT_0).await?;

    let parent_artifact = load_parent_contract_artifact();
    let child_artifact = load_child_contract_artifact();
    let child_pub_set_value_selector = child_artifact
        .find_function("pub_set_value")
        .expect("find Child.pub_set_value")
        .selector
        .expect("Child.pub_set_value selector");

    let parent_address =
        deploy_contract(&wallet, parent_artifact.clone(), default_account_address).await;
    let child_address =
        deploy_contract(&wallet, child_artifact.clone(), default_account_address).await;

    Some(TestCase {
        wallet,
        default_account_address,
        parent_address,
        parent_artifact,
        child_address,
        child_artifact,
        child_pub_set_value_selector,
    })
}

async fn send_child_method(state: &TestCase, method_name: &str) {
    let call = build_call(&state.child_artifact, state.child_address, method_name, &[]);
    state
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: state.default_account_address,
                ..Default::default()
            },
        )
        .await
        .expect("send Child method");
}

struct ProvenInteraction {
    tx_hash: TxHash,
    tx_json: serde_json::Value,
    public_function_calldata: Vec<HashedValues>,
}

async fn prove_parent_method(state: &TestCase, method_name: &str) -> ProvenInteraction {
    let call = build_call(
        &state.parent_artifact,
        state.parent_address,
        method_name,
        &[
            abi_address(state.child_address),
            abi_selector(state.child_pub_set_value_selector),
        ],
    );

    let exec = ExecutionPayload {
        calls: vec![call],
        ..Default::default()
    };
    let chain_info = state
        .wallet
        .get_chain_info()
        .await
        .expect("get chain info for proving");
    let tx_request = state
        .wallet
        .account_provider()
        .create_tx_execution_request(
            &state.default_account_address,
            exec,
            GasSettings::default(),
            &chain_info,
            None,
            None,
        )
        .await
        .expect("create tx execution request");
    let proven = state
        .wallet
        .pxe()
        .prove_tx(&tx_request, vec![state.default_account_address])
        .await
        .expect("prove tx");
    let tx_hash = proven.tx_hash.expect("prove tx to include tx hash");
    let tx_json = proven
        .to_tx()
        .to_json_value()
        .expect("serialize proven tx to json");

    ProvenInteraction {
        tx_hash,
        tx_json,
        public_function_calldata: proven.public_function_calldata,
    }
}

async fn send_proven_interaction(state: &TestCase, proven: &ProvenInteraction) {
    state
        .wallet
        .node()
        .send_tx(&proven.tx_json)
        .await
        .expect("submit proven tx to node");
    wait_for_tx(state.wallet.node(), &proven.tx_hash, WaitOpts::default())
        .await
        .expect("wait for proven tx to checkpoint");
}

fn public_call_arg_order(calldata: &[HashedValues]) -> Vec<Fr> {
    calldata
        .iter()
        .map(|entry| {
            *entry
                .values
                .get(1)
                .expect("public calldata to contain selector and first argument")
        })
        .collect()
}

fn assert_calldata_hashes_match(calldata: &[HashedValues]) {
    for entry in calldata {
        assert_eq!(entry.hash, compute_calldata_hash(&entry.values));
    }
}

// ---------------------------------------------------------------------------
// Tests: Enqueued public calls ordering
// ---------------------------------------------------------------------------

/// TS: orders public function execution in enqueue_calls_to_child_with_nested_first
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn orders_execution_enqueue_calls_nested_first() {
    let _guard = serial_guard();

    let Some(state) = setup_test_case().await else {
        return;
    };

    let expected_order = vec![Fr::from(10u64), Fr::from(20u64)];
    let proven = prove_parent_method(&state, "enqueue_calls_to_child_with_nested_first").await;

    assert_eq!(proven.public_function_calldata.len(), 2);
    assert_calldata_hashes_match(&proven.public_function_calldata);
    assert_eq!(
        public_call_arg_order(&proven.public_function_calldata),
        expected_order
    );

    send_proven_interaction(&state, &proven).await;
    expect_logs_from_last_block_to_be(state.wallet.node(), &expected_order).await;

    let value = get_child_stored_value(state.wallet.node(), state.child_address).await;
    assert_eq!(value, Fr::from(20u64));
}

/// TS: orders public function execution in enqueue_calls_to_child_with_nested_last
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn orders_execution_enqueue_calls_nested_last() {
    let _guard = serial_guard();

    let Some(state) = setup_test_case().await else {
        return;
    };

    let expected_order = vec![Fr::from(20u64), Fr::from(10u64)];
    let proven = prove_parent_method(&state, "enqueue_calls_to_child_with_nested_last").await;

    assert_eq!(proven.public_function_calldata.len(), 2);
    assert_calldata_hashes_match(&proven.public_function_calldata);
    assert_eq!(
        public_call_arg_order(&proven.public_function_calldata),
        expected_order
    );

    send_proven_interaction(&state, &proven).await;
    expect_logs_from_last_block_to_be(state.wallet.node(), &expected_order).await;

    let value = get_child_stored_value(state.wallet.node(), state.child_address).await;
    assert_eq!(value, Fr::from(10u64));
}

// ---------------------------------------------------------------------------
// Tests: Public state update ordering
// ---------------------------------------------------------------------------

/// TS: orders public state updates in set_value_twice_with_nested_first
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn orders_state_updates_set_value_twice_nested_first() {
    let _guard = serial_guard();

    let Some(state) = setup_test_case().await else {
        return;
    };

    send_child_method(&state, "set_value_twice_with_nested_first").await;

    let value = get_child_stored_value(state.wallet.node(), state.child_address).await;
    assert_eq!(value, Fr::from(20u64));
}

/// TS: orders public state updates in set_value_twice_with_nested_last
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn orders_state_updates_set_value_twice_nested_last() {
    let _guard = serial_guard();

    let Some(state) = setup_test_case().await else {
        return;
    };

    send_child_method(&state, "set_value_twice_with_nested_last").await;

    let value = get_child_stored_value(state.wallet.node(), state.child_address).await;
    assert_eq!(value, Fr::from(10u64));
}

/// TS: orders public state updates in set_value_with_two_nested_calls
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn orders_state_updates_two_nested_calls() {
    let _guard = serial_guard();

    let Some(state) = setup_test_case().await else {
        return;
    };

    send_child_method(&state, "set_value_with_two_nested_calls").await;

    let value = get_child_stored_value(state.wallet.node(), state.child_address).await;
    assert_eq!(value, Fr::from(20u64));
}

// ---------------------------------------------------------------------------
// Tests: Public log ordering
// ---------------------------------------------------------------------------

/// TS: orders public logs in set_value_twice_with_nested_first
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn orders_logs_set_value_twice_nested_first() {
    let _guard = serial_guard();

    let Some(state) = setup_test_case().await else {
        return;
    };

    let expected_order = vec![Fr::from(10u64), Fr::from(20u64)];
    send_child_method(&state, "set_value_twice_with_nested_first").await;

    expect_logs_from_last_block_to_be(state.wallet.node(), &expected_order).await;
}

/// TS: orders public logs in set_value_twice_with_nested_last
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn orders_logs_set_value_twice_nested_last() {
    let _guard = serial_guard();

    let Some(state) = setup_test_case().await else {
        return;
    };

    let expected_order = vec![Fr::from(20u64), Fr::from(10u64)];
    send_child_method(&state, "set_value_twice_with_nested_last").await;

    expect_logs_from_last_block_to_be(state.wallet.node(), &expected_order).await;
}

/// TS: orders public logs in set_value_with_two_nested_calls
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn orders_logs_two_nested_calls() {
    let _guard = serial_guard();

    let Some(state) = setup_test_case().await else {
        return;
    };

    let expected_order = vec![
        Fr::from(10u64),
        Fr::from(20u64),
        Fr::from(20u64),
        Fr::from(10u64),
        Fr::from(20u64),
    ];
    send_child_method(&state, "set_value_with_two_nested_calls").await;

    expect_logs_from_last_block_to_be(state.wallet.node(), &expected_order).await;
}
