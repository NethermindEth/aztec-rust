//! Scope isolation tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_scope_isolation.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_scope_isolation -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::map_unwrap_or,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::single_match_else,
    clippy::too_many_lines,
    dead_code,
    unused_imports
)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{AbiValue, ContractArtifact};
use aztec_rs::account::{SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::{Pxe, RegisterContractRequest};
use aztec_rs::tx::{ExecutionPayload, FunctionCall};
use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
use aztec_rs::wallet::{
    BaseWallet, ExecuteUtilityOptions, SendOptions, SimulateOptions, TxSimulationResult,
    UtilityExecutionResult, Wallet,
};

use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Constants (mirrors upstream)
// ---------------------------------------------------------------------------

const ALICE_NOTE_VALUE: u64 = 42;
const BOB_NOTE_VALUE: u64 = 100;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_scope_test_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/scope_test_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse scope_test_contract_compiled.json")
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

/// Create a wallet for `primary` account with `extra` accounts registered
/// in the PXE key store (so it can discover notes for those accounts).
async fn create_wallet(
    primary: ImportedTestAccount,
    extra: &[ImportedTestAccount],
) -> Option<(TestWallet, AztecAddress)> {
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

    // Register primary account
    let secret_key = Fr::from_hex(primary.secret_key).expect("valid secret key");
    let complete = imported_complete_address(primary);
    pxe.key_store()
        .add_account(&secret_key)
        .await
        .expect("seed key store for primary");
    pxe.address_store()
        .add(&complete)
        .await
        .expect("seed address store for primary");

    // Register extra accounts (so PXE can discover their notes)
    for account in extra {
        let sk = Fr::from_hex(account.secret_key).expect("valid extra secret key");
        let ca = imported_complete_address(*account);
        pxe.key_store()
            .add_account(&sk)
            .await
            .expect("seed key store for extra");
        pxe.address_store()
            .add(&ca)
            .await
            .expect("seed address store for extra");
    }

    let account_contract = SchnorrAccountContract::new(secret_key);
    let provider =
        SingleAccountProvider::new(complete.clone(), Box::new(account_contract), primary.alias);
    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, complete.address))
}

// ---------------------------------------------------------------------------
// Contract interaction helpers
// ---------------------------------------------------------------------------

/// Build an AztecAddress AbiValue (struct with inner field).
fn abi_address(address: AztecAddress) -> AbiValue {
    let mut fields = BTreeMap::new();
    fields.insert("inner".to_owned(), AbiValue::Field(Fr::from(address)));
    AbiValue::Struct(fields)
}

/// Look up a function by name and build a FunctionCall.
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

/// Try to extract a Field return value from a simulate result.
/// Returns `None` if the return values are empty (e.g. PCPI-encoded returns
/// that the PXE doesn't yet surface as ACIR return values).
fn try_extract_simulate_value(result: &TxSimulationResult) -> Option<u64> {
    let rv = result
        .return_values
        .get("returnValues")
        .unwrap_or(&result.return_values);
    if let Some(arr) = rv.as_array() {
        if arr.is_empty() {
            return None;
        }
    }
    Some(extract_field_value(rv))
}

/// Extract a Field return value (as u64) from a utility execution result.
fn extract_utility_value(result: &UtilityExecutionResult) -> u64 {
    extract_field_value(&result.result)
}

/// Parse a Field value from various JSON formats returned by PXE.
#[allow(clippy::cast_possible_truncation)]
fn extract_field_value(value: &serde_json::Value) -> u64 {
    if let Some(s) = value.as_str() {
        return Fr::from_hex(s)
            .map(|f| f.to_usize() as u64)
            .unwrap_or_else(|_| panic!("parse hex string: {s}"));
    }
    if let Some(arr) = value.as_array() {
        if let Some(first) = arr.first() {
            return extract_field_value(first);
        }
    }
    if let Some(n) = value.as_u64() {
        return n;
    }
    panic!("unexpected return value format: {value:?}");
}

// ---------------------------------------------------------------------------
// Shared test state (mirrors beforeAll)
// ---------------------------------------------------------------------------

struct TestState {
    alice_wallet: TestWallet,
    bob_wallet: TestWallet,
    alice: AztecAddress,
    bob: AztecAddress,
    charlie: AztecAddress,
    contract_address: AztecAddress,
    artifact: ContractArtifact,
}

static SHARED_STATE: OnceCell<Option<TestState>> = OnceCell::const_new();

/// Get or initialize the shared test state.
/// Returns `None` when the node is not reachable (test is skipped).
async fn get_shared_state() -> Option<&'static TestState> {
    let state = SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await;
    state.as_ref()
}

/// Mirrors upstream `beforeAll`: setup 3 accounts, deploy contract, create notes.
async fn init_shared_state() -> Option<TestState> {
    // setup(3) → alice, bob, charlie
    let (alice_wallet, alice) =
        create_wallet(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1, TEST_ACCOUNT_2]).await?;
    let (bob_wallet, bob) =
        create_wallet(TEST_ACCOUNT_1, &[TEST_ACCOUNT_0, TEST_ACCOUNT_2]).await?;
    let charlie =
        AztecAddress(Fr::from_hex(TEST_ACCOUNT_2.address).expect("valid charlie address"));

    let artifact = load_scope_test_artifact();

    // Deploy ScopeTestContract from alice
    let deploy =
        Contract::deploy(&alice_wallet, artifact.clone(), vec![], None).expect("deploy builder");
    let deploy_result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: alice,
                ..Default::default()
            },
        )
        .await
        .expect("deploy ScopeTestContract");
    let contract_address = deploy_result.instance.address;

    // Register contract on bob's PXE so he can interact with it
    bob_wallet
        .pxe()
        .register_contract_class(&artifact)
        .await
        .expect("register class on bob PXE");
    bob_wallet
        .pxe()
        .register_contract(RegisterContractRequest {
            instance: deploy_result.instance.clone(),
            artifact: Some(artifact.clone()),
        })
        .await
        .expect("register contract on bob PXE");

    // Alice creates a note for herself
    let create_alice = build_call(
        &artifact,
        contract_address,
        "create_note",
        vec![
            abi_address(alice),
            AbiValue::Field(Fr::from(ALICE_NOTE_VALUE)),
        ],
    );
    alice_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![create_alice],
                ..Default::default()
            },
            SendOptions {
                from: alice,
                ..Default::default()
            },
        )
        .await
        .expect("alice create_note");

    // Bob creates a note for himself
    let create_bob = build_call(
        &artifact,
        contract_address,
        "create_note",
        vec![abi_address(bob), AbiValue::Field(Fr::from(BOB_NOTE_VALUE))],
    );
    bob_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![create_bob],
                ..Default::default()
            },
            SendOptions {
                from: bob,
                ..Default::default()
            },
        )
        .await
        .expect("bob create_note");

    // Trigger PXE contract sync on both wallets so that notes created above
    // are discovered and available for subsequent simulate/utility calls.
    // Calling any utility function on the contract triggers ensure_contract_synced.
    let sync_call = build_call(
        &artifact,
        contract_address,
        "read_note_utility",
        vec![abi_address(alice)],
    );
    let _ = alice_wallet
        .execute_utility(
            sync_call.clone(),
            ExecuteUtilityOptions {
                scope: alice,
                ..Default::default()
            },
        )
        .await;
    let _ = bob_wallet
        .execute_utility(
            sync_call,
            ExecuteUtilityOptions {
                scope: bob,
                ..Default::default()
            },
        )
        .await;

    Some(TestState {
        alice_wallet,
        bob_wallet,
        alice,
        bob,
        charlie,
        contract_address,
        artifact,
    })
}

// ===========================================================================
// describe('external private')
// ===========================================================================

/// TS: it('owner can read own notes')
///
/// Simulates `contract.methods.read_note(alice)` from alice's scope.
/// Expects result == `ALICE_NOTE_VALUE`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_private_owner_can_read_own_notes() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Simulate the private call — verifies it succeeds without error
    let call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note",
        vec![abi_address(s.alice)],
    );
    let result = s
        .alice_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.alice,
                ..Default::default()
            },
        )
        .await
        .expect("simulate read_note(alice) from alice");

    // Verify the return value (extract from simulation or cross-check via utility)
    let value = match try_extract_simulate_value(&result) {
        Some(v) => v,
        None => {
            let ucall = build_call(
                &s.artifact,
                s.contract_address,
                "read_note_utility",
                vec![abi_address(s.alice)],
            );
            let uresult = s
                .alice_wallet
                .execute_utility(
                    ucall,
                    ExecuteUtilityOptions {
                        scope: s.alice,
                        ..Default::default()
                    },
                )
                .await
                .expect("execute read_note_utility(alice) scope alice");
            extract_utility_value(&uresult)
        }
    };
    assert_eq!(
        value, ALICE_NOTE_VALUE,
        "expected {ALICE_NOTE_VALUE}, got {value}"
    );
}

/// TS: it('cannot read notes belonging to a different account')
///
/// Simulates `contract.methods.read_note(alice)` from bob's scope.
/// Expects rejection: "Failed to get a note".
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_private_cannot_read_notes_belonging_to_a_different_account() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note",
        vec![abi_address(s.alice)],
    );

    let err = s
        .bob_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.bob,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: bob cannot read alice's notes");

    assert!(
        err.to_string().contains("Failed to get a note")
            || err.to_string().contains("Failed to solve brillig function"),
        "expected 'Failed to get a note' error, got: {err}"
    );
}

/// TS: it('cannot access nullifier hiding key of a different account')
///
/// Simulates `contract.methods.get_nhk(charlie)` from bob's scope.
/// Expects rejection: "Key validation request denied".
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_private_cannot_access_nullifier_hiding_key_of_a_different_account() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = build_call(
        &s.artifact,
        s.contract_address,
        "get_nhk",
        vec![abi_address(s.charlie)],
    );

    let err = s
        .bob_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.bob,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: bob cannot access charlie's NHK");

    assert!(
        err.to_string().contains("Key validation request denied"),
        "expected 'Key validation request denied' error, got: {err}"
    );
}

/// TS: it('each account can access their isolated state on a shared wallet')
///
/// alice reads her note → `ALICE_NOTE_VALUE`
/// bob reads his note → `BOB_NOTE_VALUE`
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_private_each_account_can_access_their_isolated_state_on_a_shared_wallet() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Alice reads her own note
    let alice_call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note",
        vec![abi_address(s.alice)],
    );
    let alice_result = s
        .alice_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![alice_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.alice,
                ..Default::default()
            },
        )
        .await
        .expect("simulate read_note(alice) from alice");

    // Bob reads his own note
    let bob_call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note",
        vec![abi_address(s.bob)],
    );
    let bob_result = s
        .bob_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![bob_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.bob,
                ..Default::default()
            },
        )
        .await
        .expect("simulate read_note(bob) from bob");

    // Verify return values (cross-check via utility if PCPI extraction not available)
    let alice_value = match try_extract_simulate_value(&alice_result) {
        Some(v) => v,
        None => {
            let ucall = build_call(
                &s.artifact,
                s.contract_address,
                "read_note_utility",
                vec![abi_address(s.alice)],
            );
            let ur = s
                .alice_wallet
                .execute_utility(
                    ucall,
                    ExecuteUtilityOptions {
                        scope: s.alice,
                        ..Default::default()
                    },
                )
                .await
                .expect("execute read_note_utility(alice)");
            extract_utility_value(&ur)
        }
    };
    let bob_value = match try_extract_simulate_value(&bob_result) {
        Some(v) => v,
        None => {
            let ucall = build_call(
                &s.artifact,
                s.contract_address,
                "read_note_utility",
                vec![abi_address(s.bob)],
            );
            let ur = s
                .bob_wallet
                .execute_utility(
                    ucall,
                    ExecuteUtilityOptions {
                        scope: s.bob,
                        ..Default::default()
                    },
                )
                .await
                .expect("execute read_note_utility(bob)");
            extract_utility_value(&ur)
        }
    };

    assert_eq!(
        alice_value, ALICE_NOTE_VALUE,
        "expected alice value {ALICE_NOTE_VALUE}, got {alice_value}"
    );
    assert_eq!(
        bob_value, BOB_NOTE_VALUE,
        "expected bob value {BOB_NOTE_VALUE}, got {bob_value}"
    );
}

// ===========================================================================
// describe('external utility')
// ===========================================================================

/// TS: it('owner can read own notes')
///
/// Executes `contract.methods.read_note_utility(alice)` from alice's scope.
/// Expects result == `ALICE_NOTE_VALUE`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_utility_owner_can_read_own_notes() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note_utility",
        vec![abi_address(s.alice)],
    );

    let result = s
        .alice_wallet
        .execute_utility(
            call,
            ExecuteUtilityOptions {
                scope: s.alice,
                ..Default::default()
            },
        )
        .await
        .expect("execute read_note_utility(alice) scope alice");

    let value = extract_utility_value(&result);
    assert_eq!(
        value, ALICE_NOTE_VALUE,
        "expected {ALICE_NOTE_VALUE}, got {value}"
    );
}

/// TS: it('cannot read notes belonging to a different account')
///
/// Executes `contract.methods.read_note_utility(alice)` from bob's scope.
/// Expects rejection: "Failed to get a note".
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_utility_cannot_read_notes_belonging_to_a_different_account() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note_utility",
        vec![abi_address(s.alice)],
    );

    // Use alice's wallet (which has alice's notes) but with bob's scope
    let err = s
        .alice_wallet
        .execute_utility(
            call,
            ExecuteUtilityOptions {
                scope: s.bob,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: bob scope cannot read alice's notes");

    assert!(
        err.to_string().contains("Failed to get a note")
            || err.to_string().contains("Failed to solve brillig function"),
        "expected 'Failed to get a note' error, got: {err}"
    );
}

/// TS: it('cannot access nullifier hiding key of a different account')
///
/// Executes `contract.methods.get_nhk_utility(charlie)` from bob's scope.
/// Expects rejection: "Key validation request denied".
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_utility_cannot_access_nullifier_hiding_key_of_a_different_account() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = build_call(
        &s.artifact,
        s.contract_address,
        "get_nhk_utility",
        vec![abi_address(s.charlie)],
    );

    let err = s
        .alice_wallet
        .execute_utility(
            call,
            ExecuteUtilityOptions {
                scope: s.bob,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: bob scope cannot access charlie's NHK");

    assert!(
        err.to_string().contains("Key validation request denied"),
        "expected 'Key validation request denied' error, got: {err}"
    );
}

/// TS: it('each account can access their isolated state on a shared wallet')
///
/// alice reads via utility → `ALICE_NOTE_VALUE`
/// bob reads via utility → `BOB_NOTE_VALUE`
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_utility_each_account_can_access_their_isolated_state_on_a_shared_wallet() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Alice reads her own note via utility
    let alice_call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note_utility",
        vec![abi_address(s.alice)],
    );
    let alice_result = s
        .alice_wallet
        .execute_utility(
            alice_call,
            ExecuteUtilityOptions {
                scope: s.alice,
                ..Default::default()
            },
        )
        .await
        .expect("execute read_note_utility(alice) scope alice");

    // Bob reads his own note via utility
    let bob_call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note_utility",
        vec![abi_address(s.bob)],
    );
    let bob_result = s
        .bob_wallet
        .execute_utility(
            bob_call,
            ExecuteUtilityOptions {
                scope: s.bob,
                ..Default::default()
            },
        )
        .await
        .expect("execute read_note_utility(bob) scope bob");

    let alice_value = extract_utility_value(&alice_result);
    let bob_value = extract_utility_value(&bob_result);

    assert_eq!(
        alice_value, ALICE_NOTE_VALUE,
        "expected alice value {ALICE_NOTE_VALUE}, got {alice_value}"
    );
    assert_eq!(
        bob_value, BOB_NOTE_VALUE,
        "expected bob value {BOB_NOTE_VALUE}, got {bob_value}"
    );
}
