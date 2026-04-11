//! Nested private-to-public enqueue tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_nested_contract/manual_private_enqueue.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_nested_contract_manual_private_enqueue -- --ignored --nocapture
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
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::tx::{ExecutionPayload, FunctionCall};
use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
use aztec_rs::wallet::{BaseWallet, SendOptions, Wallet};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_parent_contract_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/parent_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse parent_contract_compiled.json")
}

fn load_child_contract_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/child_contract_compiled.json");
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

fn build_call(
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
) -> FunctionCall {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|_| panic!("function '{method_name}' not found in artifact"));
    let encoded_args = encode_arguments(func, &args)
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
    child_pub_inc_value_selector: FunctionSelector,
}

async fn setup_test_case() -> Option<TestCase> {
    let (wallet, default_account_address) = setup_wallet(TEST_ACCOUNT_0).await?;

    let parent_artifact = load_parent_contract_artifact();
    let child_artifact = load_child_contract_artifact();
    let child_pub_inc_value_selector = child_artifact
        .find_function("pub_inc_value")
        .expect("find Child.pub_inc_value")
        .selector
        .expect("Child.pub_inc_value selector");

    let parent_address =
        deploy_contract(&wallet, parent_artifact.clone(), default_account_address).await;
    let child_address = deploy_contract(&wallet, child_artifact, default_account_address).await;

    Some(TestCase {
        wallet,
        default_account_address,
        parent_address,
        parent_artifact,
        child_address,
        child_pub_inc_value_selector,
    })
}

async fn send_parent_method(
    state: &TestCase,
    method_name: &str,
    child_selector: FunctionSelector,
    target_value: Option<u64>,
) {
    let mut args = vec![
        abi_address(state.child_address),
        abi_selector(child_selector),
    ];
    if let Some(value) = target_value {
        args.push(AbiValue::Field(Fr::from(value)));
    }

    let call = build_call(
        &state.parent_artifact,
        state.parent_address,
        method_name,
        args,
    );
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
        .expect("send Parent method");
}

// ---------------------------------------------------------------------------
// Tests: manual_private_enqueue
// ---------------------------------------------------------------------------

/// TS: enqueues a single public call
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn enqueues_a_single_public_call() {
    let _guard = serial_guard();

    let Some(state) = setup_test_case().await else {
        return;
    };

    send_parent_method(
        &state,
        "enqueue_call_to_child",
        state.child_pub_inc_value_selector,
        Some(42),
    )
    .await;

    let stored_value = get_child_stored_value(state.wallet.node(), state.child_address).await;
    assert_eq!(stored_value, Fr::from(42u64));
}

/// TS: enqueues multiple public calls
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn enqueues_multiple_public_calls() {
    let _guard = serial_guard();

    let Some(state) = setup_test_case().await else {
        return;
    };

    send_parent_method(
        &state,
        "enqueue_call_to_child_twice",
        state.child_pub_inc_value_selector,
        Some(42),
    )
    .await;

    let stored_value = get_child_stored_value(state.wallet.node(), state.child_address).await;
    assert_eq!(stored_value, Fr::from(85u64));
}

/// TS: enqueues a public call with nested public calls
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn enqueues_public_call_with_nested_public_calls() {
    let _guard = serial_guard();

    let Some(state) = setup_test_case().await else {
        return;
    };

    send_parent_method(
        &state,
        "enqueue_call_to_pub_entry_point",
        state.child_pub_inc_value_selector,
        Some(42),
    )
    .await;

    let stored_value = get_child_stored_value(state.wallet.node(), state.child_address).await;
    assert_eq!(stored_value, Fr::from(42u64));
}

/// TS: enqueues multiple public calls with nested public calls
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn enqueues_multiple_public_calls_with_nested() {
    let _guard = serial_guard();

    let Some(state) = setup_test_case().await else {
        return;
    };

    send_parent_method(
        &state,
        "enqueue_calls_to_pub_entry_point",
        state.child_pub_inc_value_selector,
        Some(42),
    )
    .await;

    let stored_value = get_child_stored_value(state.wallet.node(), state.child_address).await;
    assert_eq!(stored_value, Fr::from(85u64));
}
