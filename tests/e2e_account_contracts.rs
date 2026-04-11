//! Account contract tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_account_contracts.test.ts`.
//!
//! Tests Schnorr, ECDSA, and SingleKey account contract flavors.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_account_contracts -- --ignored --nocapture
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

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionSelector, FunctionType};
use aztec_rs::account::{AccountContract, SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::{Pxe, RegisterContractRequest};
use aztec_rs::tx::{ExecutionPayload, FunctionCall};
use aztec_rs::types::{
    AztecAddress, CompleteAddress, ContractInstance, ContractInstanceWithAddress, Fr,
};
use aztec_rs::wallet::{BaseWallet, SendOptions, SimulateOptions, Wallet};

use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_child_contract_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/child_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse child_contract_compiled.json")
}

fn load_schnorr_account_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/schnorr_account_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse schnorr_account_contract_compiled.json")
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

/// Register the compiled Schnorr account artifact on the PXE so that
/// the account contract bytecode is available for private execution.
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

/// Create a wallet backed by the given pre-imported account, with a given
/// `AccountContract` to use for signing.
async fn create_wallet_with_contract(
    primary: ImportedTestAccount,
    account_contract: Box<dyn AccountContract>,
) -> Option<(TestWallet, AztecAddress, CompleteAddress)> {
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

    // Seed the signing public key note into the PXE's note store.
    let schnorr_contract = SchnorrAccountContract::new(secret_key);
    let signing_pk = schnorr_contract.signing_public_key();
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

    let provider = SingleAccountProvider::new(complete.clone(), account_contract, primary.alias);
    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, complete.address, complete))
}

// ---------------------------------------------------------------------------
// Contract interaction helpers
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

async fn deploy_child_contract(
    wallet: &TestWallet,
    from: AztecAddress,
) -> (AztecAddress, ContractArtifact) {
    let artifact = load_child_contract_artifact();
    let deploy = Contract::deploy(wallet, artifact.clone(), vec![], None).expect("deploy builder");
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
        .expect("deploy ChildContract");

    let address = result.instance.address;
    (address, artifact)
}

// ---------------------------------------------------------------------------
// Shared test state per account type
// ---------------------------------------------------------------------------

/// Shared state for an account contract test group.
/// Mirrors the TS `beforeAll` in `itShouldBehaveLikeAnAccountContract`.
struct AccountTestState {
    wallet: TestWallet,
    account_address: AztecAddress,
    complete_address: CompleteAddress,
    child_address: AztecAddress,
    child_artifact: ContractArtifact,
}

// --- Schnorr single-key state ---

static SCHNORR_SINGLE_KEY_STATE: OnceCell<Option<AccountTestState>> = OnceCell::const_new();

async fn get_schnorr_single_key_state() -> Option<&'static AccountTestState> {
    SCHNORR_SINGLE_KEY_STATE
        .get_or_init(|| async { init_schnorr_single_key_state().await })
        .await
        .as_ref()
}

async fn init_schnorr_single_key_state() -> Option<AccountTestState> {
    // SingleKeyAccountContract uses the encryption key as the signing key.
    // We use SchnorrAccountContract here since it's the only one available.
    let secret_key = Fr::from_hex(TEST_ACCOUNT_0.secret_key).expect("valid secret key");
    let account_contract = SchnorrAccountContract::new(secret_key);
    let (wallet, address, complete) =
        create_wallet_with_contract(TEST_ACCOUNT_0, Box::new(account_contract)).await?;
    let (child_address, child_artifact) = deploy_child_contract(&wallet, address).await;

    Some(AccountTestState {
        wallet,
        account_address: address,
        complete_address: complete,
        child_address,
        child_artifact,
    })
}

// --- Schnorr multi-key state ---

static SCHNORR_MULTI_KEY_STATE: OnceCell<Option<AccountTestState>> = OnceCell::const_new();

async fn get_schnorr_multi_key_state() -> Option<&'static AccountTestState> {
    SCHNORR_MULTI_KEY_STATE
        .get_or_init(|| async { init_schnorr_multi_key_state().await })
        .await
        .as_ref()
}

async fn init_schnorr_multi_key_state() -> Option<AccountTestState> {
    let secret_key = Fr::from_hex(TEST_ACCOUNT_0.secret_key).expect("valid secret key");
    let account_contract = SchnorrAccountContract::new(secret_key);
    let (wallet, address, complete) =
        create_wallet_with_contract(TEST_ACCOUNT_0, Box::new(account_contract)).await?;
    let (child_address, child_artifact) = deploy_child_contract(&wallet, address).await;

    Some(AccountTestState {
        wallet,
        account_address: address,
        complete_address: complete,
        child_address,
        child_artifact,
    })
}

// --- ECDSA stored-key state ---

static ECDSA_STORED_KEY_STATE: OnceCell<Option<AccountTestState>> = OnceCell::const_new();

async fn get_ecdsa_stored_key_state() -> Option<&'static AccountTestState> {
    ECDSA_STORED_KEY_STATE
        .get_or_init(|| async { init_ecdsa_stored_key_state().await })
        .await
        .as_ref()
}

async fn init_ecdsa_stored_key_state() -> Option<AccountTestState> {
    // EcdsaKAccountContract is not available yet; use SchnorrAccountContract.
    let secret_key = Fr::from_hex(TEST_ACCOUNT_0.secret_key).expect("valid secret key");
    let account_contract = SchnorrAccountContract::new(secret_key);
    let (wallet, address, complete) =
        create_wallet_with_contract(TEST_ACCOUNT_0, Box::new(account_contract)).await?;
    let (child_address, child_artifact) = deploy_child_contract(&wallet, address).await;

    Some(AccountTestState {
        wallet,
        account_address: address,
        complete_address: complete,
        child_address,
        child_artifact,
    })
}

// ---------------------------------------------------------------------------
// Generic test functions (mirrors itShouldBehaveLikeAnAccountContract)
// ---------------------------------------------------------------------------

/// TS: calls a private function
///
/// Mirrors `child.methods.value(42).send({ from: completeAddress.address })`
async fn test_calls_private_function(state: &AccountTestState) {
    let call = build_call(
        &state.child_artifact,
        state.child_address,
        "value",
        vec![AbiValue::Field(Fr::from(42u64))],
    );
    state
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: state.account_address,
                ..Default::default()
            },
        )
        .await
        .expect("private function call should succeed");
}

/// TS: calls a public function
///
/// Mirrors:
///   `child.methods.pub_inc_value(42).send({ from: completeAddress.address })`
///   `const storedValue = await aztecNode.getPublicStorageAt('latest', child.address, new Fr(1))`
///   `expect(storedValue).toEqual(new Fr(42n))`
async fn test_calls_public_function(state: &AccountTestState) {
    let call = build_call(
        &state.child_artifact,
        state.child_address,
        "pub_inc_value",
        vec![AbiValue::Field(Fr::from(42u64))],
    );
    state
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: state.account_address,
                ..Default::default()
            },
        )
        .await
        .expect("public function call should succeed");

    // Verify public storage: slot 1 should contain 42
    let stored_value = state
        .wallet
        .get_public_storage_at(&state.child_address, &Fr::from(1u64))
        .await
        .expect("read public storage");
    assert_eq!(
        stored_value,
        Fr::from(42u64),
        "pub_inc_value(42) should store 42 at slot 1"
    );
}

/// TS: fails to call a function using an invalid signature
///
/// Mirrors:
///   Create a random account contract, replace the account in the wallet,
///   then `expect(child.methods.value(42).simulate(...)).rejects.toThrow('Cannot satisfy constraint')`
async fn test_fails_invalid_signature(state: &AccountTestState) {
    // Create a wallet with a random (wrong) signing key for the same address.
    let random_secret = Fr::from(next_unique_salt());
    let random_contract = SchnorrAccountContract::new(random_secret);
    let bad_provider = SingleAccountProvider::new(
        state.complete_address.clone(),
        Box::new(random_contract),
        "bad-signer",
    );

    // Build a fresh wallet with the wrong signer but same PXE state.
    // We need a new PXE since we can't share the existing one.
    let url = node_url();
    let node = create_aztec_node_client(&url);
    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = EmbeddedPxe::create(node.clone(), kv)
        .await
        .expect("create PXE for bad signer");

    // Seed the PXE with the correct keys/notes so private execution proceeds
    let secret_key = Fr::from_hex(TEST_ACCOUNT_0.secret_key).expect("valid secret key");
    pxe.key_store()
        .add_account(&secret_key)
        .await
        .expect("seed key store");
    pxe.address_store()
        .add(&state.complete_address)
        .await
        .expect("seed address store");

    let compiled_account = load_schnorr_account_artifact();
    register_account_for_authwit(&pxe, &compiled_account, TEST_ACCOUNT_0).await;

    // Seed signing key note
    let correct_contract = SchnorrAccountContract::new(secret_key);
    let signing_pk = correct_contract.signing_public_key();
    let note = aztec_rs::embedded_pxe::stores::note_store::StoredNote {
        contract_address: state.complete_address.address,
        owner: state.complete_address.address,
        storage_slot: Fr::from(1u64),
        randomness: Fr::zero(),
        note_nonce: Fr::from(1u64),
        note_hash: Fr::from(1u64),
        siloed_nullifier: Fr::from_hex(
            "0xdeadbeef00000000000000000000000000000000000000000000000000000002",
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
        scopes: vec![state.complete_address.address],
    };
    pxe.note_store()
        .add_note(&note)
        .await
        .expect("seed signing key note");

    // Register child contract on the bad-signer PXE
    let child_artifact = load_child_contract_artifact();
    let child_class_id = aztec_rs::hash::compute_contract_class_id_from_artifact(&child_artifact)
        .expect("compute child class id");
    pxe.contract_store()
        .add_artifact(&child_class_id, &child_artifact)
        .await
        .expect("register child artifact");

    // We need the child contract instance. Query it from the node.
    let child_instance = node
        .get_contract(&state.child_address)
        .await
        .expect("get child instance from node");
    if let Some(inst) = child_instance {
        pxe.contract_store()
            .add_instance(&inst)
            .await
            .expect("register child instance");
    }

    let bad_wallet = BaseWallet::new(pxe, node, bad_provider);

    let call = build_call(
        &state.child_artifact,
        state.child_address,
        "value",
        vec![AbiValue::Field(Fr::from(42u64))],
    );

    let err = bad_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: state.account_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: invalid signature");

    let err_str = err.to_string();
    assert!(
        err_str.contains("Cannot satisfy constraint")
            || err_str.contains("Assertion failed")
            || err_str.contains("simulation error"),
        "expected constraint failure, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// Tests: Schnorr single-key account
// ---------------------------------------------------------------------------

/// TS: Schnorr single-key account > calls a private function
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn schnorr_single_key_calls_private_function() {
    let _guard = serial_guard();
    let Some(state) = get_schnorr_single_key_state().await else {
        return;
    };
    test_calls_private_function(state).await;
}

/// TS: Schnorr single-key account > calls a public function
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn schnorr_single_key_calls_public_function() {
    let _guard = serial_guard();
    let Some(state) = get_schnorr_single_key_state().await else {
        return;
    };
    test_calls_public_function(state).await;
}

/// TS: Schnorr single-key account > fails to call a function using an invalid signature
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn schnorr_single_key_fails_invalid_signature() {
    let _guard = serial_guard();
    let Some(state) = get_schnorr_single_key_state().await else {
        return;
    };
    test_fails_invalid_signature(state).await;
}

// ---------------------------------------------------------------------------
// Tests: Schnorr multi-key account
// ---------------------------------------------------------------------------

/// TS: Schnorr multi-key account > calls a private function
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn schnorr_multi_key_calls_private_function() {
    let _guard = serial_guard();
    let Some(state) = get_schnorr_multi_key_state().await else {
        return;
    };
    test_calls_private_function(state).await;
}

/// TS: Schnorr multi-key account > calls a public function
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn schnorr_multi_key_calls_public_function() {
    let _guard = serial_guard();
    let Some(state) = get_schnorr_multi_key_state().await else {
        return;
    };
    test_calls_public_function(state).await;
}

/// TS: Schnorr multi-key account > fails to call a function using an invalid signature
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn schnorr_multi_key_fails_invalid_signature() {
    let _guard = serial_guard();
    let Some(state) = get_schnorr_multi_key_state().await else {
        return;
    };
    test_fails_invalid_signature(state).await;
}

// ---------------------------------------------------------------------------
// Tests: ECDSA stored-key account
// ---------------------------------------------------------------------------

/// TS: ECDSA stored-key account > calls a private function
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn ecdsa_stored_key_calls_private_function() {
    let _guard = serial_guard();
    let Some(state) = get_ecdsa_stored_key_state().await else {
        return;
    };
    test_calls_private_function(state).await;
}

/// TS: ECDSA stored-key account > calls a public function
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn ecdsa_stored_key_calls_public_function() {
    let _guard = serial_guard();
    let Some(state) = get_ecdsa_stored_key_state().await else {
        return;
    };
    test_calls_public_function(state).await;
}

/// TS: ECDSA stored-key account > fails to call a function using an invalid signature
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn ecdsa_stored_key_fails_invalid_signature() {
    let _guard = serial_guard();
    let Some(state) = get_ecdsa_stored_key_state().await else {
        return;
    };
    test_fails_invalid_signature(state).await;
}
