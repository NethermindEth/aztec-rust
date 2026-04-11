//! Token unshielding tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/transfer_to_public.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_transfer_to_public -- --ignored --nocapture
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
use aztec_rs::hash::{
    compute_auth_wit_message_hash, poseidon2_hash_with_separator, MessageHashOrIntent,
};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::{Pxe, RegisterContractRequest};
use aztec_rs::tx::{AuthWitness, ExecutionPayload, FunctionCall};
use aztec_rs::types::{
    AztecAddress, CompleteAddress, ContractInstance, ContractInstanceWithAddress, Fr,
};
use aztec_rs::wallet::{BaseWallet, ExecuteUtilityOptions, SendOptions, SimulateOptions, Wallet};

use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_compiled_token_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/token_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse token_contract_compiled.json")
}

fn load_generic_proxy_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/generic_proxy_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse generic_proxy_contract_compiled.json")
}

fn load_schnorr_account_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/schnorr_account_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse schnorr_account_contract_compiled.json")
}

// ---------------------------------------------------------------------------
// Constants (mirrors upstream fixtures/fixtures.ts)
// ---------------------------------------------------------------------------

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

/// Register the compiled Schnorr account artifact on the PXE so that
/// authwit utility functions can be executed locally.
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

/// Create a wallet with account artifact registered for authwit.
async fn create_wallet(
    primary: ImportedTestAccount,
    extra: &[ImportedTestAccount],
) -> Option<(TestWallet, AztecAddress)> {
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
        .expect("seed key store for primary");
    pxe.address_store()
        .add(&complete)
        .await
        .expect("seed address store for primary");

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

    // Register compiled Schnorr account artifact for authwit utility.
    let compiled_account = load_schnorr_account_artifact();
    register_account_for_authwit(&pxe, &compiled_account, primary).await;
    for account in extra {
        register_account_for_authwit(&pxe, &compiled_account, *account).await;
    }

    let account_contract = SchnorrAccountContract::new(secret_key);

    // Seed the signing public key note into the PXE's note store.
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

    // Register senders for tag discovery
    for account in extra {
        let addr = AztecAddress(Fr::from_hex(account.address).expect("valid address"));
        pxe.register_sender(&addr)
            .await
            .expect("register sender for extra");
    }

    let provider =
        SingleAccountProvider::new(complete.clone(), Box::new(account_contract), primary.alias);
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
    fields.insert(
        "inner".to_owned(),
        AbiValue::Integer(u32::from_be_bytes(selector.0).into()),
    );
    AbiValue::Struct(fields)
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

fn build_proxy_call(
    proxy_artifact: &ContractArtifact,
    proxy_address: AztecAddress,
    action: &FunctionCall,
) -> FunctionCall {
    let arg_count = action.args.len();
    let method_name = format!("forward_private_{arg_count}");
    let consume_selector = action.selector;

    build_call(
        proxy_artifact,
        proxy_address,
        &method_name,
        vec![
            abi_address(action.to),
            abi_selector(consume_selector),
            AbiValue::Array(
                action
                    .args
                    .iter()
                    .map(|a| match a {
                        AbiValue::Field(f) => AbiValue::Field(*f),
                        other => other.clone(),
                    })
                    .collect(),
            ),
        ],
    )
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
    pxe.register_contract_class(artifact)
        .await
        .unwrap_or_else(|e| eprintln!("register class: {e}"));
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

/// Call a utility (view) function and parse the result as a `u128`.
#[allow(clippy::cast_possible_truncation)]
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
// Public storage helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Shared test state (mirrors beforeAll in upstream TokenContractTest)
// ---------------------------------------------------------------------------

struct TestState {
    admin_wallet: TestWallet,
    admin_address: AztecAddress,
    account1_address: AztecAddress,
    token_address: AztecAddress,
    token_artifact: ContractArtifact,
    proxy_address: AztecAddress,
    proxy_artifact: ContractArtifact,
}

static SHARED_STATE: OnceCell<Option<TestState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static TestState> {
    let state = SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await;
    state.as_ref()
}

async fn init_shared_state() -> Option<TestState> {
    let (admin_wallet, admin_address) = create_wallet(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1]).await?;

    let account1_complete = imported_complete_address(TEST_ACCOUNT_1);
    let account1_address = account1_complete.address;

    eprintln!("admin:    {admin_address}");
    eprintln!("account1: {account1_address}");

    // Deploy token contract
    eprintln!("deploying Token from admin...");
    let (token_address, token_artifact, _token_instance) = deploy_contract(
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
    eprintln!("Token deployed at {token_address}");

    // Deploy GenericProxy contract
    eprintln!("deploying GenericProxy from admin...");
    let (proxy_address, proxy_artifact, _proxy_instance) = deploy_contract(
        &admin_wallet,
        load_generic_proxy_artifact(),
        vec![],
        admin_address,
    )
    .await;
    eprintln!("GenericProxy deployed at {proxy_address}");

    // Mint public tokens to admin (mirrors upstream applyMintSnapshot)
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

    // Mint private tokens to admin
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

    Some(TestState {
        admin_wallet,
        admin_address,
        account1_address,
        token_address,
        token_artifact,
        proxy_address,
        proxy_artifact,
    })
}

// ===========================================================================
// Tests: e2e_token_contract transfer_to_public (unshielding)
// ===========================================================================

/// TS: on behalf of self
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_public_on_behalf_of_self() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let priv_before = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = priv_before / 2;
    assert!(amount > 0, "admin should have a positive private balance");
    eprintln!("transfer_to_public on behalf of self: amount={amount} (priv_before={priv_before})");

    let pub_before = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;

    send_token_method(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "transfer_to_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(0), // nonce = 0 for self
        ],
        s.admin_address,
    )
    .await;

    wait_for_next_block(&s.admin_wallet).await;

    let priv_after = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    assert_eq!(
        priv_after,
        priv_before - amount,
        "admin private balance should decrease by {amount}"
    );

    let pub_after = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    assert_eq!(
        pub_after,
        pub_before + amount,
        "admin public balance should increase by {amount}"
    );
}

/// TS: on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_public_on_behalf_of_other() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let priv_before = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = priv_before / 2;
    let authwit_nonce = Fr::random();
    assert!(amount > 0, "admin should have a positive private balance");
    eprintln!(
        "transfer_to_public on behalf of other: amount={amount} (priv_before={priv_before})"
    );

    let account1_pub_before =
        public_balance(&s.admin_wallet, s.token_address, &s.account1_address).await;

    // Build the transfer_to_public action
    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_to_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Create authwit: admin authorizes proxy to call transfer_to_public
    let intent = MessageHashOrIntent::Intent {
        caller: s.proxy_address,
        call: action.clone(),
    };
    let witness = s
        .admin_wallet
        .create_auth_wit(s.admin_address, intent)
        .await
        .expect("create authwit");

    // Admin sends through proxy so their keys are in scope, while proxy
    // becomes msg_sender to trigger authwit.
    let proxy_call = build_proxy_call(&s.proxy_artifact, s.proxy_address, &action);
    s.admin_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![proxy_call.clone()],
                ..Default::default()
            },
            SendOptions {
                from: s.admin_address,
                auth_witnesses: vec![witness.clone()],
                ..Default::default()
            },
        )
        .await
        .expect("send transfer_to_public via proxy");

    wait_for_next_block(&s.admin_wallet).await;

    let priv_after = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    assert_eq!(
        priv_after,
        priv_before - amount,
        "admin private balance should decrease"
    );

    let account1_pub_after =
        public_balance(&s.admin_wallet, s.token_address, &s.account1_address).await;
    assert_eq!(
        account1_pub_after,
        account1_pub_before + amount,
        "account1 public balance should increase"
    );

    // Perform the transfer again — should fail (duplicate nullifier)
    let err = s
        .admin_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![proxy_call],
                ..Default::default()
            },
            SendOptions {
                from: s.admin_address,
                auth_witnesses: vec![witness],
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: duplicate nullifier");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("duplicate nullifier")
            || err_str.contains("duplicate siloed nullifier")
            || err_str.contains("nullifier already exists")
            || err_str.contains("nullifier collision")
            || err_str.contains("existing nullifier"),
        "expected duplicate nullifier error, got: {err}"
    );
}

// -- failure cases --

/// TS: failure cases > on behalf of self (more than balance)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_public_on_behalf_of_self_more_than_balance() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let priv_balance = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = priv_balance + 1;
    assert!(amount > 0);
    eprintln!(
        "transfer_to_public more than balance: amount={amount} (balance={priv_balance})"
    );

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_to_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(0),
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
        .expect_err("should fail: balance too low");

    let err_str = err.to_string();
    assert!(
        err_str.contains("Balance too low")
            || err_str.contains("Assertion failed")
            || err_str.contains("Cannot satisfy constraint"),
        "expected 'Balance too low' or constraint failure, got: {err}"
    );
}

/// TS: failure cases > on behalf of self (invalid authwit nonce)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_public_on_behalf_of_self_invalid_authwit_nonce() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let priv_balance = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = priv_balance + 1;
    assert!(amount > 0);
    eprintln!("transfer_to_public invalid nonce: amount={amount}");

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_to_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(1), // non-zero nonce when from == msg_sender
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
        .expect_err("should fail: invalid authwit nonce");

    let err_str = err.to_string();
    assert!(
        err_str.contains("Invalid authwit nonce")
            || err_str.contains("Assertion failed")
            || err_str.contains("Cannot satisfy constraint"),
        "expected 'Invalid authwit nonce' or constraint failure, got: {err}"
    );
}

/// TS: failure cases > on behalf of other (more than balance)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_public_on_behalf_of_other_more_than_balance() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let priv_balance = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = priv_balance + 2;
    let authwit_nonce = Fr::random();
    assert!(amount > 0);
    eprintln!(
        "transfer_to_public on behalf of other more than balance: amount={amount} (balance={priv_balance})"
    );

    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_to_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    let intent = MessageHashOrIntent::Intent {
        caller: s.proxy_address,
        call: action.clone(),
    };
    let witness = s
        .admin_wallet
        .create_auth_wit(s.admin_address, intent)
        .await
        .expect("create authwit");

    // Admin sends through proxy — simulate only
    let proxy_call = build_proxy_call(&s.proxy_artifact, s.proxy_address, &action);
    let err = s
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![proxy_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.admin_address,
                auth_witnesses: vec![witness],
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: balance too low");

    let err_str = err.to_string();
    assert!(
        err_str.contains("Balance too low")
            || err_str.contains("Assertion failed")
            || err_str.contains("Cannot satisfy constraint"),
        "expected 'Balance too low' or constraint failure, got: {err}"
    );
}

/// TS: failure cases > on behalf of other (invalid designated caller)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_public_on_behalf_of_other_invalid_designated_caller() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let priv_balance = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = priv_balance + 2;
    let authwit_nonce = Fr::random();
    assert!(amount > 0);
    eprintln!("transfer_to_public invalid designated caller: amount={amount}");

    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_to_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );

    // Compute the expected message hash (proxy as caller, which is what the
    // contract will check for)
    let chain_info = s
        .admin_wallet
        .get_chain_info()
        .await
        .expect("get chain info");
    let expected_message_hash = compute_auth_wit_message_hash(
        &MessageHashOrIntent::Intent {
            caller: s.proxy_address,
            call: action.clone(),
        },
        &chain_info,
    );

    // Create authwit with WRONG caller (account1 instead of proxy)
    let wrong_intent = MessageHashOrIntent::Intent {
        caller: s.account1_address,
        call: action.clone(),
    };
    let witness = s
        .admin_wallet
        .create_auth_wit(s.admin_address, wrong_intent)
        .await
        .expect("create authwit with wrong caller");

    // Admin sends through proxy — authwit is for wrong caller
    let proxy_call = build_proxy_call(&s.proxy_artifact, s.proxy_address, &action);
    let err = s
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![proxy_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.admin_address,
                auth_witnesses: vec![witness],
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: wrong designated caller");

    let err_str = err.to_string();
    assert!(
        err_str.contains(&format!(
            "Unknown auth witness for message hash {expected_message_hash}"
        )) || err_str.contains("Unknown auth witness")
            || err_str.contains("auth witness")
            || err_str.contains("Cannot satisfy constraint")
            || err_str.contains("execution failed"),
        "expected auth witness error or constraint failure, got: {err}"
    );
}
