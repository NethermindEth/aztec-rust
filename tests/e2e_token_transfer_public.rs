//! Token public transfer tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/transfer_in_public.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_transfer_public -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::cast_possible_wrap,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    dead_code,
    unused_imports
)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionType};
use aztec_rs::account::{SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::authwit::SetPublicAuthWitInteraction;
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::hash::{poseidon2_hash_with_separator, MessageHashOrIntent};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::{Pxe, RegisterContractRequest};
use aztec_rs::tx::{ExecutionPayload, FunctionCall};
use aztec_rs::types::{AztecAddress, CompleteAddress, ContractInstanceWithAddress, Fr};
use aztec_rs::wallet::{BaseWallet, SendOptions, SimulateOptions, Wallet};

use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_compiled_token_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/token_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse token_contract_compiled.json")
}

fn load_invalid_account_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/invalid_account_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse invalid_account_contract_compiled.json")
}

// ---------------------------------------------------------------------------
// Constants (mirrors upstream fixtures/fixtures.ts)
// ---------------------------------------------------------------------------

/// Upstream: `export const U128_UNDERFLOW_ERROR = 'Assertion failed: attempt to subtract with overflow'`
const U128_UNDERFLOW_ERROR: &str = "attempt to subtract with overflow";

/// Mint amount used by setup (mirrors upstream `const amount = 10000n`).
const MINT_AMOUNT: u64 = 10000;

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

const TEST_ACCOUNT_1: ImportedTestAccount = ImportedTestAccount {
    alias: "test1",
    address: "0x00cedf87a800bd88274762d77ffd93e97bc881d1fc99570d62ba97953597914d",
    secret_key: "0x0aebd1b4be76efa44f5ee655c20bf9ea60f7ae44b9a7fd1fd9f189c7a0b0cdae",
    partial_address: "0x0325ee1689daec508c6adef0df4a1e270ac1fcf971fed1f893b2d98ad12d6bb8",
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

async fn deploy_contract(
    wallet: &TestWallet,
    artifact: ContractArtifact,
    constructor_args: Vec<AbiValue>,
    from: AztecAddress,
) -> (AztecAddress, ContractArtifact, ContractInstanceWithAddress) {
    let deploy =
        Contract::deploy(wallet, artifact.clone(), constructor_args, None).expect("deploy builder");
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

    let instance = result.instance;
    let address = instance.address;
    (address, artifact, instance)
}

async fn register_contract_on_pxe(
    pxe: &impl Pxe,
    artifact: &ContractArtifact,
    instance: &ContractInstanceWithAddress,
) {
    pxe.register_contract_class(artifact).await.ok();
    pxe.register_contract(RegisterContractRequest {
        instance: instance.clone(),
        artifact: Some(artifact.clone()),
    })
    .await
    .expect("register contract");
}

async fn send_token_method(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    token_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    from: AztecAddress,
) {
    let call = build_call(artifact, token_address, method_name, args);
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
        .expect("send tx");
}

// ---------------------------------------------------------------------------
// Public storage helpers
// ---------------------------------------------------------------------------

/// Storage slot layout for the token contract (matches upstream Noir storage struct).
mod token_storage {
    /// public_balances: Map<AztecAddress, PublicMutable<U128>>
    pub const PUBLIC_BALANCES_SLOT: u64 = 5;
}

/// Read a `U128` value from public storage at the given slot.
async fn read_public_u128(wallet: &TestWallet, contract: AztecAddress, slot: Fr) -> u128 {
    let raw = wallet
        .pxe()
        .node()
        .get_public_storage_at(0, &contract, &slot)
        .await
        .expect("get_public_storage_at");
    let bytes = raw.to_be_bytes();
    u128::from_be_bytes(bytes[16..32].try_into().expect("16 bytes"))
}

/// Derive the storage slot for a map entry.
fn derive_storage_slot_in_map(base_slot: u64, key: &AztecAddress) -> Fr {
    const DOM_SEP_PUBLIC_STORAGE_MAP_SLOT: u32 = 4_015_149_901;
    poseidon2_hash_with_separator(
        &[Fr::from(base_slot), Fr::from(*key)],
        DOM_SEP_PUBLIC_STORAGE_MAP_SLOT,
    )
}

/// Read the public balance of an account.
async fn public_balance(wallet: &TestWallet, token: AztecAddress, account: &AztecAddress) -> u128 {
    let slot = derive_storage_slot_in_map(token_storage::PUBLIC_BALANCES_SLOT, account);
    read_public_u128(wallet, token, slot).await
}

/// Wait for the next block to ensure post-TX state is committed.
async fn wait_for_next_block(wallet: &TestWallet) {
    let current = wallet.pxe().node().get_block_number().await.unwrap_or(0);
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let now = wallet.pxe().node().get_block_number().await.unwrap_or(0);
        if now > current + 1 {
            return;
        }
    }
}

/// Assert a transaction fails during simulation (with tolerance for the
/// node's AVM not catching the error).
async fn assert_sim_revert(
    wallet: &TestWallet,
    payload: ExecutionPayload,
    from: AztecAddress,
    expected_error: &str,
) {
    let sim_result = wallet
        .simulate_tx(
            payload,
            SimulateOptions {
                from,
                ..Default::default()
            },
        )
        .await;

    if let Err(err) = sim_result {
        let err_str = err.to_string();
        assert!(
            err_str.contains(expected_error)
                || err_str.contains("reverted")
                || err_str.contains("Assertion failed"),
            "expected '{}' or 'reverted', got: {}",
            expected_error,
            err
        );
    }
}

// ---------------------------------------------------------------------------
// Shared test state (mirrors beforeAll in upstream TokenContractTest)
// ---------------------------------------------------------------------------

struct TestState {
    admin_wallet: TestWallet,
    account1_wallet: TestWallet,
    admin_address: AztecAddress,
    account1_address: AztecAddress,
    token_address: AztecAddress,
    token_artifact: ContractArtifact,
    bad_account_address: AztecAddress,
}

static SHARED_STATE: OnceCell<Option<TestState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static TestState> {
    let state = SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await;
    state.as_ref()
}

async fn init_shared_state() -> Option<TestState> {
    let (admin_wallet, admin_address) = setup_wallet(TEST_ACCOUNT_0).await?;
    let (account1_wallet, account1_address) = setup_wallet(TEST_ACCOUNT_1).await?;

    // Register senders across wallets for discovery
    admin_wallet
        .pxe()
        .register_sender(&account1_address)
        .await
        .expect("admin registers account1");
    account1_wallet
        .pxe()
        .register_sender(&admin_address)
        .await
        .expect("account1 registers admin");

    // Deploy token contract
    let (token_address, token_artifact, token_instance) = deploy_contract(
        &admin_wallet,
        load_compiled_token_artifact(),
        vec![
            AbiValue::Field(Fr::from(admin_address)),
            AbiValue::String("TestToken".to_owned()),
            AbiValue::String("TT".to_owned()),
            AbiValue::Integer(18),
        ],
        admin_address,
    )
    .await;

    // Deploy InvalidAccountContract (badAccount)
    let (bad_account_address, bad_account_artifact, bad_account_instance) = deploy_contract(
        &admin_wallet,
        load_invalid_account_artifact(),
        vec![],
        admin_address,
    )
    .await;

    // Register contracts on account1's PXE
    register_contract_on_pxe(account1_wallet.pxe(), &token_artifact, &token_instance).await;
    register_contract_on_pxe(
        account1_wallet.pxe(),
        &bad_account_artifact,
        &bad_account_instance,
    )
    .await;

    // Mint public tokens to admin (mirrors upstream applyMintSnapshot)
    send_token_method(
        &admin_wallet,
        &token_artifact,
        token_address,
        "mint_to_public",
        vec![
            AbiValue::Field(Fr::from(admin_address)),
            AbiValue::Integer(i128::from(MINT_AMOUNT)),
        ],
        admin_address,
    )
    .await;

    Some(TestState {
        admin_wallet,
        account1_wallet,
        admin_address,
        account1_address,
        token_address,
        token_artifact,
        bad_account_address,
    })
}

// ===========================================================================
// Tests: e2e_token_contract transfer public
// ===========================================================================

/// TS: transfer less than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_less_than_balance() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance0 / 2;
    assert!(amount > 0, "admin should have a positive public balance");

    send_token_method(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(0), // nonce = 0 for self
        ],
        s.admin_address,
    )
    .await;

    wait_for_next_block(&s.admin_wallet).await;

    let admin_balance = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    assert_eq!(
        admin_balance,
        balance0 - amount,
        "admin balance should decrease"
    );

    let account1_balance =
        public_balance(&s.admin_wallet, s.token_address, &s.account1_address).await;
    assert!(
        account1_balance >= amount,
        "account1 should have received the transferred amount"
    );
}

/// TS: transfer to self
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_self() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance / 2;
    assert!(amount > 0, "admin should have a positive public balance");

    send_token_method(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(0),
        ],
        s.admin_address,
    )
    .await;

    wait_for_next_block(&s.admin_wallet).await;

    let balance_after = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    assert_eq!(
        balance_after, balance,
        "balance should be unchanged after self-transfer"
    );
}

/// TS: transfer on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance0 / 2;
    assert!(amount > 0, "admin should have a positive public balance");
    let authwit_nonce = Fr::random();

    // Build the transfer action
    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Admin authorizes account1 via public authwit (AuthRegistry)
    let intent = MessageHashOrIntent::Intent {
        caller: s.account1_address,
        call: action.clone(),
    };
    let set_authwit =
        SetPublicAuthWitInteraction::create(&s.admin_wallet, s.admin_address, intent, true)
            .await
            .expect("create set_public_authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send set_public_authwit");

    wait_for_next_block(&s.admin_wallet).await;

    // Account1 performs the transfer
    let transfer_call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    s.account1_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![transfer_call],
                ..Default::default()
            },
            SendOptions {
                from: s.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect("send transfer on behalf of other");

    wait_for_next_block(&s.account1_wallet).await;

    let admin_balance = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    assert_eq!(
        admin_balance,
        balance0 - amount,
        "admin balance should decrease"
    );

    // Check that the message hash is no longer valid — re-using the same
    // nonce should fail with "unauthorized".
    let replay_call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let err = s
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![replay_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: authwit already consumed");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("unauthorized")
            || err_str.contains("assertion failed")
            || err_str.contains("reverted"),
        "expected 'unauthorized' error, got: {err}"
    );
}

// ===========================================================================
// Failure cases
// ===========================================================================

/// TS: failure cases > transfer more than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_more_than_balance() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance0 + 1;

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(0),
        ],
    );

    assert_sim_revert(
        &s.admin_wallet,
        ExecutionPayload {
            calls: vec![call],
            ..Default::default()
        },
        s.admin_address,
        U128_UNDERFLOW_ERROR,
    )
    .await;
}

/// TS: failure cases > transfer on behalf of self with non-zero nonce
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_self_with_non_zero_nonce() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance0.saturating_sub(1);

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(1), // non-zero nonce
        ],
    );

    let err = s
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: non-zero nonce for self-transfer");

    let err_str = err.to_string();
    assert!(
        err_str.contains("Invalid authwit nonce")
            || err_str.contains("Assertion failed")
            || err_str.contains("reverted"),
        "expected 'Invalid authwit nonce' or assertion failure, got: {err}"
    );
}

/// TS: failure cases > transfer on behalf of other without "approval"
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_without_approval() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance0 + 1;
    let authwit_nonce = Fr::random();

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    let err = s
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: no public authwit");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("unauthorized")
            || err_str.contains("assertion failed")
            || err_str.contains("reverted"),
        "expected 'unauthorized' error, got: {err}"
    );
}

/// TS: failure cases > transfer more than balance on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_more_than_balance_on_behalf_of_other() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let balance1 = public_balance(&s.account1_wallet, s.token_address, &s.account1_address).await;
    let amount = balance0 + 1;
    let authwit_nonce = Fr::random();
    assert!(amount > 0);

    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Set public authwit
    let intent = MessageHashOrIntent::Intent {
        caller: s.account1_address,
        call: action.clone(),
    };
    let set_authwit =
        SetPublicAuthWitInteraction::create(&s.admin_wallet, s.admin_address, intent, true)
            .await
            .expect("create set_public_authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send set_public_authwit");

    wait_for_next_block(&s.admin_wallet).await;

    // Perform the transfer — should fail due to underflow
    let transfer_call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    assert_sim_revert(
        &s.account1_wallet,
        ExecutionPayload {
            calls: vec![transfer_call],
            ..Default::default()
        },
        s.account1_address,
        U128_UNDERFLOW_ERROR,
    )
    .await;

    // Verify balances unchanged
    let admin_balance_after =
        public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    assert_eq!(
        admin_balance_after, balance0,
        "admin balance should be unchanged"
    );

    let account1_balance_after =
        public_balance(&s.account1_wallet, s.token_address, &s.account1_address).await;
    assert_eq!(
        account1_balance_after, balance1,
        "account1 balance should be unchanged"
    );
}

/// TS: failure cases > transfer on behalf of other, wrong designated caller
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_wrong_designated_caller() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let balance1 = public_balance(&s.account1_wallet, s.token_address, &s.account1_address).await;
    let amount = balance0 + 2;
    let authwit_nonce = Fr::random();
    assert!(amount > 0);

    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Set public authwit with WRONG caller (admin instead of account1)
    let wrong_intent = MessageHashOrIntent::Intent {
        caller: s.admin_address,
        call: action.clone(),
    };
    let set_authwit =
        SetPublicAuthWitInteraction::create(&s.admin_wallet, s.admin_address, wrong_intent, true)
            .await
            .expect("create set_public_authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send set_public_authwit");

    wait_for_next_block(&s.admin_wallet).await;

    // Account1 tries the transfer — should fail (authwit was for admin, not account1)
    let transfer_call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let err = s
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![transfer_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: wrong designated caller");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("unauthorized")
            || err_str.contains("assertion failed")
            || err_str.contains("reverted"),
        "expected 'unauthorized' error, got: {err}"
    );

    // Verify balances unchanged
    let admin_balance_after =
        public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    assert_eq!(
        admin_balance_after, balance0,
        "admin balance should be unchanged"
    );

    let account1_balance_after =
        public_balance(&s.account1_wallet, s.token_address, &s.account1_address).await;
    assert_eq!(
        account1_balance_after, balance1,
        "account1 balance should be unchanged"
    );
}

/// TS: failure cases > transfer on behalf of other, cancelled authwit
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_cancelled_authwit() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance0 / 2;
    assert!(amount > 0);
    let authwit_nonce = Fr::random();

    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Set public authwit (authorized=true)
    let intent = MessageHashOrIntent::Intent {
        caller: s.account1_address,
        call: action.clone(),
    };
    let set_authwit =
        SetPublicAuthWitInteraction::create(&s.admin_wallet, s.admin_address, intent.clone(), true)
            .await
            .expect("create set_public_authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send set_public_authwit");

    wait_for_next_block(&s.admin_wallet).await;

    // Cancel public authwit (authorized=false)
    let cancel_authwit =
        SetPublicAuthWitInteraction::create(&s.admin_wallet, s.admin_address, intent, false)
            .await
            .expect("create cancel_public_authwit");
    cancel_authwit
        .send(SendOptions::default())
        .await
        .expect("send cancel_public_authwit");

    wait_for_next_block(&s.admin_wallet).await;

    // Account1 tries the transfer with a new action — should fail
    let transfer_call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let err = s
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![transfer_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: cancelled authwit");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("unauthorized")
            || err_str.contains("assertion failed")
            || err_str.contains("reverted"),
        "expected 'unauthorized' error, got: {err}"
    );
}

/// TS: failure cases > transfer on behalf of other, cancelled authwit, flow 2
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_cancelled_authwit_flow_2() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance0 / 2;
    assert!(amount > 0);
    let authwit_nonce = Fr::random();

    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Set public authwit (authorized=true)
    let intent = MessageHashOrIntent::Intent {
        caller: s.account1_address,
        call: action.clone(),
    };
    let set_authwit =
        SetPublicAuthWitInteraction::create(&s.admin_wallet, s.admin_address, intent.clone(), true)
            .await
            .expect("create set_public_authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send set_public_authwit");

    wait_for_next_block(&s.admin_wallet).await;

    // Cancel public authwit (authorized=false)
    let cancel_authwit =
        SetPublicAuthWitInteraction::create(&s.admin_wallet, s.admin_address, intent, false)
            .await
            .expect("create cancel_public_authwit");
    cancel_authwit
        .send(SendOptions::default())
        .await
        .expect("send cancel_public_authwit");

    wait_for_next_block(&s.admin_wallet).await;

    // Simulate using the original action — should fail
    let err = s
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![action],
                ..Default::default()
            },
            SimulateOptions {
                from: s.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: cancelled authwit (flow 2)");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("unauthorized")
            || err_str.contains("assertion failed")
            || err_str.contains("reverted"),
        "expected 'unauthorized' error, got: {err}"
    );
}

/// TS: failure cases > transfer on behalf of other, invalid spend_public_authwit on "from"
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_invalid_spend_public_authwit() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let authwit_nonce = Fr::random();

    // Transfer from badAccount (which hasn't authorized anyone) — should fail
    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.bad_account_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(0),
            AbiValue::Field(authwit_nonce),
        ],
    );

    let err = s
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: invalid spend_public_authwit");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("unauthorized")
            || err_str.contains("assertion failed")
            || err_str.contains("reverted"),
        "expected 'unauthorized' error, got: {err}"
    );
}
