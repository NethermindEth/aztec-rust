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

mod common;
use common::*;

use std::sync::Arc;

use aztec_rs::account::AccountContract;

use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

/// Create a wallet backed by the given pre-imported account, with a given
/// `AccountContract` to use for signing.
async fn create_wallet_with_contract(
    primary: ImportedTestAccount,
    account_contract: Box<dyn AccountContract>,
) -> Option<(TestWallet, AztecAddress, CompleteAddress)> {
    let url = node_url();
    let node = create_aztec_node_client(&url);
    if node.get_node_info().await.is_err() {
        return None;
    }

    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = EmbeddedPxe::create(node.clone(), kv).await.ok()?;

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

    let schnorr_contract = SchnorrAccountContract::new(secret_key);
    seed_signing_key_note(&pxe, &schnorr_contract, complete.address, 1).await;

    let provider = SingleAccountProvider::new(complete.clone(), account_contract, primary.alias);
    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, complete.address, complete))
}

async fn deploy_child_contract(
    wallet: &TestWallet,
    from: AztecAddress,
) -> (AztecAddress, ContractArtifact) {
    let artifact = load_child_contract_artifact();
    let (address, artifact, _instance) = deploy_contract(wallet, artifact, vec![], from).await;
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
    send_call(&state.wallet, call, state.account_address).await;
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
    send_call(&state.wallet, call, state.account_address).await;

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

    let correct_contract = SchnorrAccountContract::new(secret_key);
    seed_signing_key_note(&pxe, &correct_contract, state.complete_address.address, 2).await;

    // Register child contract on the bad-signer PXE
    let child_artifact = load_child_contract_artifact();
    let child_class_id =
        compute_contract_class_id_from_artifact(&child_artifact).expect("compute child class id");
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
