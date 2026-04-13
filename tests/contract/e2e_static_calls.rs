//! Static call tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_static_calls.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_static_calls -- --ignored --nocapture
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
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionSelector};
use aztec_rs::account::{SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::node::{create_aztec_node_client, wait_for_tx, AztecNode, HttpNodeClient, WaitOpts};
use aztec_rs::tx::{ExecutionPayload, FunctionCall, TxStatus};
use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
use aztec_rs::wallet::{BaseWallet, SendOptions, SimulateOptions, Wallet};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_static_parent_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/static_parent_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse static_parent_contract_compiled.json")
}

fn load_static_child_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/static_child_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse static_child_contract_compiled.json")
}

const STATIC_CALL_STATE_MODIFICATION_ERROR: &str = "Static call cannot update the state";
const STATIC_CONTEXT_ASSERTION_ERROR: &str = "can only be called statically";

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
    assert_eq!(complete.address, expected_address);
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
    pxe.key_store()
        .add_account(&secret_key)
        .await
        .expect("seed key store");
    pxe.address_store()
        .add(&complete)
        .await
        .expect("seed address store");

    let provider = SingleAccountProvider::new(
        complete.clone(),
        Box::new(SchnorrAccountContract::new(secret_key)),
        account.alias,
    );
    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, complete.address))
}

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
    Contract::deploy(wallet, artifact, vec![], None)
        .expect("deploy builder")
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
        .expect("deploy contract")
        .instance
        .address
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
    FunctionCall {
        to: contract_address,
        selector: func.selector.expect("selector"),
        args,
        function_type: func.function_type.clone(),
        is_static: func.is_static,
        hide_msg_sender: false,
    }
}

async fn send_call(
    wallet: &TestWallet,
    call: FunctionCall,
    from: AztecAddress,
) -> Result<(), aztec_rs::Error> {
    send_call_with_wait_status(wallet, call, from, TxStatus::Checkpointed).await
}

async fn send_call_expect_revert(
    wallet: &TestWallet,
    call: FunctionCall,
    from: AztecAddress,
) -> Result<(), aztec_rs::Error> {
    let result = wallet
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
        .await?;

    let start = tokio::time::Instant::now();
    let timeout = Duration::from_secs(120);
    let interval = Duration::from_secs(1);
    let mut last_status = None;
    let mut last_error = None;

    loop {
        if start.elapsed() >= timeout {
            let status =
                last_status.map_or_else(|| "unknown".to_owned(), |status| format!("{status:?}"));
            let error = last_error.unwrap_or_else(|| "unknown reason".to_owned());
            return Err(aztec_rs::Error::Timeout(format!(
                "tx {} did not produce a terminal execution result within {:?} (last status: {}, last error: {})",
                result.tx_hash, timeout, status, error
            )));
        }

        if let Ok(receipt) = wallet.node().get_tx_receipt(&result.tx_hash).await {
            last_status = Some(receipt.status);
            last_error.clone_from(&receipt.error);

            if receipt.is_dropped() {
                return Err(aztec_rs::Error::Reverted(format!(
                    "tx {} was dropped: {}",
                    result.tx_hash,
                    receipt.error.as_deref().unwrap_or("unknown reason")
                )));
            }

            if receipt.execution_result.is_some() {
                if receipt.has_execution_reverted() {
                    return Err(aztec_rs::Error::Reverted(format!(
                        "tx {} execution reverted: {}",
                        result.tx_hash,
                        receipt.error.as_deref().unwrap_or("unknown reason")
                    )));
                }
                return Ok(());
            }
        }

        tokio::time::sleep(interval).await;
    }
}

async fn send_call_with_wait_status(
    wallet: &TestWallet,
    call: FunctionCall,
    from: AztecAddress,
    wait_for_status: TxStatus,
) -> Result<(), aztec_rs::Error> {
    let result = wallet
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
        .await?;

    wait_for_tx(
        wallet.node(),
        &result.tx_hash,
        WaitOpts {
            wait_for_status,
            timeout: Duration::from_secs(120),
            interval: Duration::from_secs(1),
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

async fn simulate_call(
    wallet: &TestWallet,
    call: FunctionCall,
    from: AztecAddress,
) -> Result<(), aztec_rs::Error> {
    wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from,
                ..Default::default()
            },
        )
        .await
        .map(|_| ())
}

fn contains_static_state_error(err: &aztec_rs::Error) -> bool {
    let err = err.to_string();
    err.contains(STATIC_CALL_STATE_MODIFICATION_ERROR)
        || err.contains("Static call cannot update the state")
        || err.contains("reverted")
}

fn contains_static_context_error(err: &aztec_rs::Error) -> bool {
    let err = err.to_string();
    err.contains(STATIC_CONTEXT_ASSERTION_ERROR)
        || err.contains("can only be called statically")
        || err.contains("reverted")
}

struct TestCase {
    wallet: TestWallet,
    owner: AztecAddress,
    sender: AztecAddress,
    parent_address: AztecAddress,
    child_address: AztecAddress,
    parent: ContractArtifact,
    child: ContractArtifact,
}

async fn setup_test_case() -> Option<TestCase> {
    let (wallet, owner) = setup_wallet(TEST_ACCOUNT_0).await?;
    let sender = owner;
    let parent = load_static_parent_artifact();
    let child = load_static_child_artifact();
    let parent_address = deploy_contract(&wallet, parent.clone(), owner).await;
    let child_address = deploy_contract(&wallet, child.clone(), owner).await;

    send_call(
        &wallet,
        build_call(
            &child,
            child_address,
            "private_set_value",
            vec![
                AbiValue::Field(Fr::from(42u64)),
                abi_address(owner),
                abi_address(sender),
            ],
        ),
        owner,
    )
    .await
    .expect("seed private note");

    Some(TestCase {
        wallet,
        owner,
        sender,
        parent_address,
        child_address,
        parent,
        child,
    })
}

// ---------------------------------------------------------------------------
// Tests: Direct view calls to child
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn performs_legal_private_static_calls() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    send_call(
        &s.wallet,
        build_call(
            &s.child,
            s.child_address,
            "private_get_value",
            vec![AbiValue::Field(Fr::from(42u64)), abi_address(s.owner)],
        ),
        s.owner,
    )
    .await
    .expect("private static call should succeed");
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn fails_non_static_calls_to_poorly_written_static_private() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let err = send_call_expect_revert(
        &s.wallet,
        build_call(
            &s.child,
            s.child_address,
            "private_illegal_set_value",
            vec![AbiValue::Field(Fr::from(42u64)), abi_address(s.owner)],
        ),
        s.owner,
    )
    .await
    .expect_err("illegal private static call should fail");
    assert!(contains_static_state_error(&err));
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn performs_legal_public_static_calls() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    send_call(
        &s.wallet,
        build_call(
            &s.child,
            s.child_address,
            "pub_get_value",
            vec![AbiValue::Field(Fr::from(42u64))],
        ),
        s.owner,
    )
    .await
    .expect("public static call should succeed");
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn fails_non_static_calls_to_poorly_written_static_public() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let err = simulate_call(
        &s.wallet,
        build_call(
            &s.child,
            s.child_address,
            "pub_illegal_inc_value",
            vec![AbiValue::Field(Fr::from(42u64))],
        ),
        s.owner,
    )
    .await
    .expect_err("illegal public static call should fail");
    assert!(contains_static_state_error(&err));
}

// ---------------------------------------------------------------------------
// Tests: Parent calls child
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn performs_legal_private_to_private_static_calls() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let child_private_get = s
        .child
        .find_function("private_get_value")
        .expect("private_get_value")
        .selector
        .expect("private_get_value selector");

    send_call(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "private_static_call",
            vec![
                abi_address(s.child_address),
                abi_selector(child_private_get),
                AbiValue::Array(vec![
                    AbiValue::Field(Fr::from(42u64)),
                    AbiValue::Field(Fr::from(s.owner)),
                ]),
            ],
        ),
        s.owner,
    )
    .await
    .expect("low-level private static call");

    send_call(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "private_get_value_from_child",
            vec![
                abi_address(s.child_address),
                AbiValue::Field(Fr::from(42u64)),
                abi_address(s.owner),
            ],
        ),
        s.owner,
    )
    .await
    .expect("interface private static call");
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn performs_legal_nested_private_to_private_static_calls() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let child_private_get = s
        .child
        .find_function("private_get_value")
        .expect("private_get_value")
        .selector
        .expect("private_get_value selector");

    send_call(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "private_nested_static_call",
            vec![
                abi_address(s.child_address),
                abi_selector(child_private_get),
                AbiValue::Array(vec![
                    AbiValue::Field(Fr::from(42u64)),
                    AbiValue::Field(Fr::from(s.owner)),
                ]),
            ],
        ),
        s.owner,
    )
    .await
    .expect("nested private static call");
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn performs_legal_public_to_public_static_calls() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let child_pub_get = s
        .child
        .find_function("pub_get_value")
        .expect("pub_get_value")
        .selector
        .expect("pub_get_value selector");

    send_call(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "public_static_call",
            vec![
                abi_address(s.child_address),
                abi_selector(child_pub_get),
                AbiValue::Array(vec![AbiValue::Field(Fr::from(42u64))]),
            ],
        ),
        s.owner,
    )
    .await
    .expect("low-level public static call");

    send_call(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "public_get_value_from_child",
            vec![
                abi_address(s.child_address),
                AbiValue::Field(Fr::from(42u64)),
            ],
        ),
        s.owner,
    )
    .await
    .expect("interface public static call");
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn performs_legal_nested_public_to_public_static_calls() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let child_pub_get = s
        .child
        .find_function("pub_get_value")
        .expect("pub_get_value")
        .selector
        .expect("pub_get_value selector");

    send_call(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "public_nested_static_call",
            vec![
                abi_address(s.child_address),
                abi_selector(child_pub_get),
                AbiValue::Array(vec![AbiValue::Field(Fr::from(42u64))]),
            ],
        ),
        s.owner,
    )
    .await
    .expect("nested public static call");
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn performs_legal_enqueued_public_static_calls() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let child_pub_get = s
        .child
        .find_function("pub_get_value")
        .expect("pub_get_value")
        .selector
        .expect("pub_get_value selector");

    send_call(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "enqueue_static_call_to_pub_function",
            vec![
                abi_address(s.child_address),
                abi_selector(child_pub_get),
                AbiValue::Array(vec![AbiValue::Field(Fr::from(42u64))]),
            ],
        ),
        s.owner,
    )
    .await
    .expect("enqueue public static call");

    send_call(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "enqueue_public_get_value_from_child",
            vec![
                abi_address(s.child_address),
                AbiValue::Field(Fr::from(42u64)),
            ],
        ),
        s.owner,
    )
    .await
    .expect("enqueue interface public static call");
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn performs_legal_nested_enqueued_public_static_calls() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let child_pub_get = s
        .child
        .find_function("pub_get_value")
        .expect("pub_get_value")
        .selector
        .expect("pub_get_value selector");

    send_call(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "enqueue_static_nested_call_to_pub_function",
            vec![
                abi_address(s.child_address),
                abi_selector(child_pub_get),
                AbiValue::Array(vec![AbiValue::Field(Fr::from(42u64))]),
            ],
        ),
        s.owner,
    )
    .await
    .expect("nested enqueue public static call");
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn fails_illegal_private_to_private_static_calls() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let child_private_set = s
        .child
        .find_function("private_set_value")
        .expect("private_set_value")
        .selector
        .expect("private_set_value selector");

    let err = send_call_expect_revert(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "private_static_call_3_args",
            vec![
                abi_address(s.child_address),
                abi_selector(child_private_set),
                AbiValue::Array(vec![
                    AbiValue::Field(Fr::from(42u64)),
                    AbiValue::Field(Fr::from(s.owner)),
                    AbiValue::Field(Fr::from(s.sender)),
                ]),
            ],
        ),
        s.owner,
    )
    .await
    .expect_err("illegal private static call should fail");
    assert!(contains_static_state_error(&err));
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn fails_non_static_calls_to_poorly_written_private_static() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let child_illegal_private = s
        .child
        .find_function("private_illegal_set_value")
        .expect("private_illegal_set_value")
        .selector
        .expect("private_illegal_set_value selector");

    let err = send_call_expect_revert(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "private_call",
            vec![
                abi_address(s.child_address),
                abi_selector(child_illegal_private),
                AbiValue::Array(vec![
                    AbiValue::Field(Fr::from(42u64)),
                    AbiValue::Field(Fr::from(s.owner)),
                ]),
            ],
        ),
        s.owner,
    )
    .await
    .expect_err("non-static private call should fail");
    assert!(contains_static_context_error(&err));
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn fails_illegal_nested_private_to_private_static_calls() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let child_private_set = s
        .child
        .find_function("private_set_value")
        .expect("private_set_value")
        .selector
        .expect("private_set_value selector");

    let err = send_call_expect_revert(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "private_nested_static_call_3_args",
            vec![
                abi_address(s.child_address),
                abi_selector(child_private_set),
                AbiValue::Array(vec![
                    AbiValue::Field(Fr::from(42u64)),
                    AbiValue::Field(Fr::from(s.owner)),
                    AbiValue::Field(Fr::from(s.sender)),
                ]),
            ],
        ),
        s.owner,
    )
    .await
    .expect_err("nested illegal private static call should fail");
    assert!(contains_static_state_error(&err));
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn fails_illegal_public_to_public_static_calls() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let child_pub_set = s
        .child
        .find_function("pub_set_value")
        .expect("pub_set_value")
        .selector
        .expect("pub_set_value selector");

    let err = simulate_call(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "public_static_call",
            vec![
                abi_address(s.child_address),
                abi_selector(child_pub_set),
                AbiValue::Array(vec![AbiValue::Field(Fr::from(42u64))]),
            ],
        ),
        s.owner,
    )
    .await
    .expect_err("illegal public static call should fail");
    assert!(contains_static_state_error(&err));
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn fails_illegal_nested_public_to_public_static_calls() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let child_pub_set = s
        .child
        .find_function("pub_set_value")
        .expect("pub_set_value")
        .selector
        .expect("pub_set_value selector");

    let err = simulate_call(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "public_nested_static_call",
            vec![
                abi_address(s.child_address),
                abi_selector(child_pub_set),
                AbiValue::Array(vec![AbiValue::Field(Fr::from(42u64))]),
            ],
        ),
        s.owner,
    )
    .await
    .expect_err("illegal nested public static call should fail");
    assert!(contains_static_state_error(&err));
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn fails_illegal_enqueued_public_static_calls() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let child_pub_set = s
        .child
        .find_function("pub_set_value")
        .expect("pub_set_value")
        .selector
        .expect("pub_set_value selector");

    let err = simulate_call(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "enqueue_static_call_to_pub_function",
            vec![
                abi_address(s.child_address),
                abi_selector(child_pub_set),
                AbiValue::Array(vec![AbiValue::Field(Fr::from(42u64))]),
            ],
        ),
        s.owner,
    )
    .await
    .expect_err("illegal enqueued public static call should fail");
    assert!(contains_static_state_error(&err));
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn fails_illegal_nested_enqueued_public_static_calls() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let child_pub_set = s
        .child
        .find_function("pub_set_value")
        .expect("pub_set_value")
        .selector
        .expect("pub_set_value selector");

    let err = simulate_call(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "enqueue_static_nested_call_to_pub_function",
            vec![
                abi_address(s.child_address),
                abi_selector(child_pub_set),
                AbiValue::Array(vec![AbiValue::Field(Fr::from(42u64))]),
            ],
        ),
        s.owner,
    )
    .await
    .expect_err("illegal nested enqueued public static call should fail");
    assert!(contains_static_state_error(&err));
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn fails_non_static_enqueue_to_poorly_written_public_static() {
    let _guard = serial_guard();
    let Some(s) = setup_test_case().await else {
        return;
    };
    let child_pub_illegal = s
        .child
        .find_function("pub_illegal_inc_value")
        .expect("pub_illegal_inc_value")
        .selector
        .expect("pub_illegal_inc_value selector");

    let err = simulate_call(
        &s.wallet,
        build_call(
            &s.parent,
            s.parent_address,
            "enqueue_call",
            vec![
                abi_address(s.child_address),
                abi_selector(child_pub_illegal),
                AbiValue::Array(vec![AbiValue::Field(Fr::from(42u64))]),
            ],
        ),
        s.owner,
    )
    .await
    .expect_err("non-static enqueue to static function should fail");
    assert!(contains_static_context_error(&err));
}
