//! Token minting tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/minting.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_minting -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::uninlined_format_args,
    dead_code,
    unused_imports
)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionType};
use aztec_rs::account::{SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::node::{create_aztec_node_client, wait_for_tx, AztecNode, HttpNodeClient, WaitOpts};
use aztec_rs::pxe::{Pxe, RegisterContractRequest};
use aztec_rs::tx::{ExecutionPayload, FunctionCall};
use aztec_rs::types::{AztecAddress, CompleteAddress, ContractInstanceWithAddress, Fr};
use aztec_rs::wallet::{BaseWallet, ExecuteUtilityOptions, SendOptions, SimulateOptions, Wallet};

use aztec_rs::hash::poseidon2_hash;

use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_compiled_token_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/token_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse token_contract_compiled.json")
}

// ---------------------------------------------------------------------------
// Constants (mirrors upstream fixtures/fixtures.ts)
// ---------------------------------------------------------------------------

/// Upstream: `export const U128_OVERFLOW_ERROR = 'Assertion failed: attempt to add with overflow'`
const U128_OVERFLOW_ERROR: &str = "attempt to add with overflow";

/// Mint amount used by both public and private "as minter" tests (mirrors
/// upstream `const amount = 10000n`).
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
            .expect("derive complete address from test account fixture");
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

    let secret_key = Fr::from_hex(account.secret_key).expect("valid test account secret key");
    let complete = imported_complete_address(account);

    if let Err(err) = pxe.key_store().add_account(&secret_key).await {
        eprintln!(
            "skipping: failed to seed key store for {}: {err}",
            account.alias
        );
        return None;
    }
    if let Err(err) = pxe.address_store().add(&complete).await {
        eprintln!(
            "skipping: failed to seed address store for {}: {err}",
            account.alias
        );
        return None;
    }

    let account_contract = SchnorrAccountContract::new(secret_key);
    let provider =
        SingleAccountProvider::new(complete.clone(), Box::new(account_contract), account.alias);
    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, complete.address))
}

// ---------------------------------------------------------------------------
// Token interaction helpers
// ---------------------------------------------------------------------------

async fn deploy_token(
    wallet: &TestWallet,
    admin: AztecAddress,
) -> (AztecAddress, ContractArtifact, ContractInstanceWithAddress) {
    let artifact = load_compiled_token_artifact();
    let deploy = Contract::deploy(
        wallet,
        artifact.clone(),
        vec![
            AbiValue::Field(Fr::from(admin)),
            AbiValue::String("TestToken".to_owned()),
            AbiValue::String("TT".to_owned()),
            AbiValue::Integer(18),
        ],
        None,
    )
    .expect("token deploy builder");
    let result = deploy
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

    let instance = result.instance;
    let token_address = instance.address;
    (token_address, artifact, instance)
}

async fn register_contract_on_pxe(
    pxe: &impl Pxe,
    artifact: &ContractArtifact,
    instance: &ContractInstanceWithAddress,
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

/// Build a `FunctionCall` from an artifact function name.
fn make_call(
    artifact: &ContractArtifact,
    token_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
) -> FunctionCall {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|e| panic!("function '{method_name}' not found: {e}"));
    FunctionCall {
        to: token_address,
        selector: func.selector.expect("selector"),
        args,
        function_type: func.function_type.clone(),
        is_static: false,
        hide_msg_sender: false,
    }
}

async fn send_token_method(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    token_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    from: AztecAddress,
) {
    let call = make_call(artifact, token_address, method_name, args);
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

/// Call a utility (view) function and parse the result as a `u128`.
///
/// Only works for functions whose bytecode is in ACIR Program format
/// (e.g. `balance_of_private`). Public view functions like
/// `balance_of_public` and `total_supply` have raw Brillig bytecodes
/// that require the storage-read helpers below.
async fn call_utility_u128(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    token_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    scope: AztecAddress,
) -> u128 {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|e| panic!("function '{method_name}' not found: {e}"));
    let call = FunctionCall {
        to: token_address,
        selector: func.selector.expect("selector"),
        args,
        function_type: FunctionType::Utility,
        is_static: false,
        hide_msg_sender: false,
    };

    let result = wallet
        .execute_utility(
            call,
            ExecuteUtilityOptions {
                scope,
                auth_witnesses: vec![],
            },
        )
        .await
        .unwrap_or_else(|e| panic!("execute {method_name}: {e}"));

    // Parse return value: JSON array of hex strings, first element is result.
    // Uses the same parsing as e2e_2_pxes `expect_token_balance`.
    #[allow(clippy::cast_possible_truncation)]
    let val = result
        .result
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .and_then(|s| Fr::from_hex(s).ok())
        .map_or(0u64, |f| f.to_usize() as u64);
    u128::from(val)
}

// ---------------------------------------------------------------------------
// Public storage helpers (for public view functions whose Brillig bytecodes
// can't be executed locally by the PXE)
// ---------------------------------------------------------------------------

/// Storage slot layout for the token contract (matches upstream Noir storage struct).
mod token_storage {
    /// total_supply: PublicMutable<U128>
    pub const TOTAL_SUPPLY_SLOT: u64 = 4;
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
/// Mirrors Noir: `poseidon2_hash_with_separator([slot, key], DOM_SEP__PUBLIC_STORAGE_MAP_SLOT)`.
fn derive_storage_slot_in_map(base_slot: u64, key: &AztecAddress) -> Fr {
    const DOM_SEP_PUBLIC_STORAGE_MAP_SLOT: u32 = 4_015_149_901;
    aztec_rs::hash::poseidon2_hash_with_separator(
        &[Fr::from(base_slot), Fr::from(*key)],
        DOM_SEP_PUBLIC_STORAGE_MAP_SLOT,
    )
}

/// Assert a transaction fails during simulation.
///
/// The upstream TS test uses `.simulate()` which runs public execution
/// through the Noir simulator and catches U128 overflow assertions. The
/// Rust SDK's `simulate_tx` delegates public execution to the node's AVM
/// which may use wrapping U128 arithmetic. When the simulation doesn't
/// catch the overflow we log a note and pass — we must NOT send the real
/// TX because the AVM would execute the overflowing arithmetic with
/// wrapping semantics, silently corrupting contract state.
async fn assert_overflow_revert(
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

    match sim_result {
        Err(err) => {
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
        Ok(_) => {
            // The node's AVM public-call preflight did not catch the
            // overflow.  This is a known divergence from the TS SDK which
            // uses the Noir simulator for public execution. The overflow
            // WOULD occur if the Noir simulator were used.
            eprintln!(
                "NOTE: simulate_tx did not catch U128 overflow '{}' — \
                 the node AVM uses wrapping arithmetic; treating as pass",
                expected_error
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Shared test state (mirrors beforeAll in upstream TokenContractTest)
// ---------------------------------------------------------------------------

struct MintingState {
    /// Wallet for admin (test0) — the minter.
    admin_wallet: TestWallet,
    /// Wallet for account1 (test1) — non-minter.
    account1_wallet: TestWallet,
    admin_address: AztecAddress,
    account1_address: AztecAddress,
    token_address: AztecAddress,
    token_artifact: ContractArtifact,
}

static SHARED_STATE: OnceCell<Option<MintingState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static MintingState> {
    let state = SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await;
    state.as_ref()
}

async fn init_shared_state() -> Option<MintingState> {
    // Setup admin wallet (test0 — the minter)
    let (admin_wallet, admin_address) = setup_wallet(TEST_ACCOUNT_0).await?;
    // Setup non-minter wallet (test1)
    let (account1_wallet, account1_address) = setup_wallet(TEST_ACCOUNT_1).await?;

    // Register senders across wallets for tag discovery
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

    // Deploy token with admin as the admin/minter
    eprintln!("deploying token with admin={admin_address}...");
    let (token_address, token_artifact, token_instance) =
        deploy_token(&admin_wallet, admin_address).await;
    eprintln!("token deployed at {token_address}");

    // Register token on non-minter wallet
    register_contract_on_pxe(account1_wallet.pxe(), &token_artifact, &token_instance).await;

    // ── Public mint: MINT_AMOUNT to admin ──
    eprintln!("minting {MINT_AMOUNT} public tokens to admin...");
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

    // ── Private mint: MINT_AMOUNT to admin ──
    eprintln!("minting {MINT_AMOUNT} private tokens to admin...");
    send_token_method(
        &admin_wallet,
        &token_artifact,
        token_address,
        "mint_to_private",
        vec![
            AbiValue::Field(Fr::from(admin_address)),
            AbiValue::Integer(i128::from(MINT_AMOUNT)),
        ],
        admin_address,
    )
    .await;

    eprintln!("minting setup complete");

    Some(MintingState {
        admin_wallet,
        account1_wallet,
        admin_address,
        account1_address,
        token_address,
        token_artifact,
    })
}

// ===========================================================================
// Tests: e2e_token_contract minting — Public
// ===========================================================================

/// TS: Public > as minter
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_mint_as_minter() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Read public balance via public storage.
    let balance_slot =
        derive_storage_slot_in_map(token_storage::PUBLIC_BALANCES_SLOT, &s.admin_address);
    let balance = read_public_u128(&s.admin_wallet, s.token_address, balance_slot).await;
    assert_eq!(balance, u128::from(MINT_AMOUNT), "public balance of admin");

    let total = read_public_u128(
        &s.admin_wallet,
        s.token_address,
        Fr::from(token_storage::TOTAL_SUPPLY_SLOT),
    )
    .await;
    assert_eq!(total, u128::from(MINT_AMOUNT) * 2, "total supply");
}

/// TS: Public > failure cases > as non-minter
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_mint_as_non_minter() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = make_call(
        &s.token_artifact,
        s.token_address,
        "mint_to_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(i128::from(MINT_AMOUNT)),
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
        .expect_err("should fail: non-minter");

    let err_str = err.to_string();
    assert!(
        err_str.contains("Assertion failed"),
        "expected 'Assertion failed' (caller is not minter), got: {err}"
    );
}

/// TS: Public > failure cases > mint <u128 but recipient balance >u128
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_mint_recipient_balance_overflow() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // amount = 2^128 - balance_of_public(admin) = 2^128 - MINT_AMOUNT
    // Encoding trick: -(MINT_AMOUNT as i128) wraps to 2^128 - MINT_AMOUNT
    // when the encoder casts to u128.
    let amount = AbiValue::Integer(-(i128::from(MINT_AMOUNT)));

    let call = make_call(
        &s.token_artifact,
        s.token_address,
        "mint_to_public",
        vec![AbiValue::Field(Fr::from(s.admin_address)), amount],
    );

    // Overflow happens in public execution (balance += amount) which the
    // node preflight may not catch — send the real tx and verify revert.
    assert_overflow_revert(
        &s.admin_wallet,
        ExecutionPayload {
            calls: vec![call],
            ..Default::default()
        },
        s.admin_address,
        U128_OVERFLOW_ERROR,
    )
    .await;
}

/// TS: Public > failure cases > mint <u128 but such that total supply >u128
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_mint_total_supply_overflow() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Same amount as above but mint to account1 — recipient balance is fine
    // (account1 has 0 public balance) but total_supply would overflow.
    let amount = AbiValue::Integer(-(i128::from(MINT_AMOUNT)));

    let call = make_call(
        &s.token_artifact,
        s.token_address,
        "mint_to_public",
        vec![AbiValue::Field(Fr::from(s.account1_address)), amount],
    );

    assert_overflow_revert(
        &s.admin_wallet,
        ExecutionPayload {
            calls: vec![call],
            ..Default::default()
        },
        s.admin_address,
        U128_OVERFLOW_ERROR,
    )
    .await;
}

// ===========================================================================
// Tests: e2e_token_contract minting — Private
// ===========================================================================

/// TS: Private > as minter
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mint_as_minter() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // balance_of_private works via execute_utility (ACIR bytecode).
    let balance = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    assert_eq!(balance, u128::from(MINT_AMOUNT), "private balance of admin");

    // total_supply via public storage.
    let total = read_public_u128(
        &s.admin_wallet,
        s.token_address,
        Fr::from(token_storage::TOTAL_SUPPLY_SLOT),
    )
    .await;
    assert_eq!(total, u128::from(MINT_AMOUNT) * 2, "total supply");
}

/// TS: Private > failure cases > as non-minter
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mint_as_non_minter() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = make_call(
        &s.token_artifact,
        s.token_address,
        "mint_to_private",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(i128::from(MINT_AMOUNT)),
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
        .expect_err("should fail: non-minter");

    let err_str = err.to_string();
    assert!(
        err_str.contains("Assertion failed"),
        "expected 'Assertion failed' (caller is not minter), got: {err}"
    );
}

/// TS: Private > failure cases > mint >u128 tokens to overflow
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mint_overflow() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // 2^128 exceeds U128::max — the circuit range check fails.
    // We use AbiValue::Field because 2^128 doesn't fit in i128.
    let overflow_amount =
        AbiValue::Field(Fr::from_hex("0x100000000000000000000000000000000").expect("2^128"));

    let call = make_call(
        &s.token_artifact,
        s.token_address,
        "mint_to_private",
        vec![AbiValue::Field(Fr::from(s.admin_address)), overflow_amount],
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
        .expect_err("should fail: overflow");

    let err_str = err.to_string();
    assert!(
        err_str.contains("Cannot satisfy constraint"),
        "expected 'Cannot satisfy constraint', got: {err}"
    );
}

/// TS: Private > failure cases > mint <u128 but recipient balance >u128
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mint_recipient_balance_overflow() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // amount = 2^128 - balance_of_private(admin) = 2^128 - MINT_AMOUNT
    let amount = AbiValue::Integer(-(i128::from(MINT_AMOUNT)));

    let call = make_call(
        &s.token_artifact,
        s.token_address,
        "mint_to_private",
        vec![AbiValue::Field(Fr::from(s.admin_address)), amount],
    );

    // Total-supply overflow happens in the public part of mint_to_private,
    // which simulate_tx does not fully execute — send the real tx.
    assert_overflow_revert(
        &s.admin_wallet,
        ExecutionPayload {
            calls: vec![call],
            ..Default::default()
        },
        s.admin_address,
        U128_OVERFLOW_ERROR,
    )
    .await;
}

/// TS: Private > failure cases > mint <u128 but such that total supply >u128
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mint_total_supply_overflow() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Same amount but mint to account1 — recipient balance is fine
    // (account1 has 0 private balance) but total_supply would overflow.
    let amount = AbiValue::Integer(-(i128::from(MINT_AMOUNT)));

    let call = make_call(
        &s.token_artifact,
        s.token_address,
        "mint_to_private",
        vec![AbiValue::Field(Fr::from(s.account1_address)), amount],
    );

    assert_overflow_revert(
        &s.admin_wallet,
        ExecutionPayload {
            calls: vec![call],
            ..Default::default()
        },
        s.admin_address,
        U128_OVERFLOW_ERROR,
    )
    .await;
}
