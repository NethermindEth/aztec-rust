//! Token burn tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/burn.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_burn -- --ignored --nocapture
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
    dead_code,
    unused_imports
)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionSelector, FunctionType};
use aztec_rs::account::{AccountContract, SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::authwit::SetPublicAuthWitInteraction;
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::hash::{
    compute_auth_wit_message_hash, poseidon2_hash_with_separator, MessageHashOrIntent,
};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::{Pxe, RegisterContractRequest};
use aztec_rs::tx::{ExecutionPayload, FunctionCall};
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

const U128_UNDERFLOW_ERROR: &str = "attempt to subtract with overflow";
const DUPLICATE_NULLIFIER_ERROR: &str = "nullifier";
const MINT_AMOUNT: u64 = 10000;

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

type InnerWallet = BaseWallet<EmbeddedPxe<HttpNodeClient>, HttpNodeClient, SingleAccountProvider>;
type TestWallet = Arc<InnerWallet>;

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
    assert_eq!(complete.address, expected_address);
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

    let secret_key = Fr::from_hex(primary.secret_key).expect("valid primary secret key");
    let complete = imported_complete_address(primary);
    pxe.key_store()
        .add_account(&secret_key)
        .await
        .expect("seed key store");
    pxe.address_store()
        .add(&complete)
        .await
        .expect("seed address store");

    for account in extra {
        let sk = Fr::from_hex(account.secret_key).expect("valid extra secret key");
        let ca = imported_complete_address(*account);
        pxe.key_store()
            .add_account(&sk)
            .await
            .expect("seed extra key");
        pxe.address_store()
            .add(&ca)
            .await
            .expect("seed extra address");
        pxe.register_sender(&ca.address)
            .await
            .expect("register sender");
    }

    let compiled_account = load_schnorr_account_artifact();
    register_account_for_authwit(&pxe, &compiled_account, primary).await;
    for account in extra {
        register_account_for_authwit(&pxe, &compiled_account, *account).await;
    }

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
        .expect("seed signing note");

    let provider = SingleAccountProvider::new(
        complete.clone(),
        Box::new(SchnorrAccountContract::new(secret_key)),
        primary.alias,
    );
    let wallet = Arc::new(BaseWallet::new(pxe, node, provider));
    Some((wallet, complete.address))
}

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
    FunctionCall {
        to: contract_address,
        selector: func.selector.expect("selector"),
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
    let method_name = format!("forward_private_{}", action.args.len());
    build_call(
        proxy_artifact,
        proxy_address,
        &method_name,
        vec![
            abi_address(action.to),
            abi_selector(action.selector),
            AbiValue::Array(action.args.clone()),
        ],
    )
}

async fn deploy_contract(
    wallet: &TestWallet,
    artifact: ContractArtifact,
    constructor_args: Vec<AbiValue>,
    from: AztecAddress,
) -> (AztecAddress, ContractArtifact, ContractInstanceWithAddress) {
    let result = Contract::deploy(wallet, artifact.clone(), constructor_args, None)
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
        .expect("deploy contract");

    (result.instance.address, artifact, result.instance)
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

mod token_storage {
    pub const PUBLIC_BALANCES_SLOT: u64 = 5;
}

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

fn derive_storage_slot_in_map(base_slot: u64, key: &AztecAddress) -> Fr {
    const DOM_SEP_PUBLIC_STORAGE_MAP_SLOT: u32 = 4_015_149_901;
    poseidon2_hash_with_separator(
        &[Fr::from(base_slot), Fr::from(*key)],
        DOM_SEP_PUBLIC_STORAGE_MAP_SLOT,
    )
}

async fn public_balance(wallet: &TestWallet, token: AztecAddress, account: &AztecAddress) -> u128 {
    let slot = derive_storage_slot_in_map(token_storage::PUBLIC_BALANCES_SLOT, account);
    read_public_u128(wallet, token, slot).await
}

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
// Shared test state (mirrors TokenContractTest base + mint snapshots)
// ---------------------------------------------------------------------------

struct TestState {
    admin_wallet: TestWallet,
    account1_wallet: TestWallet,
    admin_address: AztecAddress,
    account1_address: AztecAddress,
    token_address: AztecAddress,
    token_artifact: ContractArtifact,
    proxy_address: AztecAddress,
    proxy_artifact: ContractArtifact,
}

static SHARED_STATE: OnceCell<Option<TestState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static TestState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<TestState> {
    let (admin_wallet, admin_address) = create_wallet(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1]).await?;
    let (account1_wallet, account1_address) =
        create_wallet(TEST_ACCOUNT_1, &[TEST_ACCOUNT_0]).await?;

    let (token_address, token_artifact, token_instance) = deploy_contract(
        &admin_wallet,
        load_compiled_token_artifact(),
        vec![
            AbiValue::Field(Fr::from(admin_address)),
            AbiValue::String("USDC".to_owned()),
            AbiValue::String("USD".to_owned()),
            AbiValue::Integer(18),
        ],
        admin_address,
    )
    .await;

    let (proxy_address, proxy_artifact, proxy_instance) = deploy_contract(
        &admin_wallet,
        load_generic_proxy_artifact(),
        vec![],
        admin_address,
    )
    .await;

    register_contract_on_pxe(account1_wallet.pxe(), &token_artifact, &token_instance).await;
    register_contract_on_pxe(account1_wallet.pxe(), &proxy_artifact, &proxy_instance).await;

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

    Some(TestState {
        admin_wallet,
        account1_wallet,
        admin_address,
        account1_address,
        token_address,
        token_artifact,
        proxy_address,
        proxy_artifact,
    })
}

// ---------------------------------------------------------------------------
// Tests: Public burn
// ---------------------------------------------------------------------------

/// TS: public > burn less than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_burn_less_than_balance() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance0 / 2;
    assert!(amount > 0);

    send_token_method(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "burn_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(0),
        ],
        s.admin_address,
    )
    .await;

    wait_for_next_block(&s.admin_wallet).await;
    let balance_after = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    assert_eq!(balance_after, balance0 - amount);
}

/// TS: public > burn on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_burn_on_behalf_of_other() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance0 / 2;
    let authwit_nonce = Fr::random();
    assert!(amount > 0);

    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "burn_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let intent = MessageHashOrIntent::Intent {
        caller: s.account1_address,
        call: action.clone(),
    };
    let set_authwit =
        SetPublicAuthWitInteraction::create(&s.admin_wallet, s.admin_address, intent, true)
            .await
            .expect("create public authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send public authwit");

    s.account1_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![action.clone()],
                ..Default::default()
            },
            SendOptions {
                from: s.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect("burn public on behalf");

    wait_for_next_block(&s.account1_wallet).await;
    let balance_after = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    assert_eq!(balance_after, balance0 - amount);

    let replay_err = s
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
        .expect_err("replay should fail");
    assert!(replay_err
        .to_string()
        .to_lowercase()
        .contains("unauthorized"));
}

/// TS: public > burn more than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_burn_more_than_balance_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance0 + 1;
    let err = s
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &s.token_artifact,
                    s.token_address,
                    "burn_public",
                    vec![
                        AbiValue::Field(Fr::from(s.admin_address)),
                        AbiValue::Integer(amount as i128),
                        AbiValue::Integer(0),
                    ],
                )],
                ..Default::default()
            },
            SimulateOptions {
                from: s.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("burn more than balance should fail");
    assert!(err.to_string().contains(U128_UNDERFLOW_ERROR));
}

/// TS: public > burn on behalf of self with non-zero nonce
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_burn_self_nonzero_nonce_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance0 - 1;
    let err = s
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &s.token_artifact,
                    s.token_address,
                    "burn_public",
                    vec![
                        AbiValue::Field(Fr::from(s.admin_address)),
                        AbiValue::Integer(amount as i128),
                        AbiValue::Integer(1),
                    ],
                )],
                ..Default::default()
            },
            SimulateOptions {
                from: s.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("non-zero nonce on self burn should fail");
    assert!(err.to_string().contains("Invalid authwit nonce"));
}

/// TS: public > burn on behalf of other without "approval"
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_burn_on_behalf_without_approval_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance0 + 1;
    let authwit_nonce = Fr::random();
    let err = s
        .account1_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &s.token_artifact,
                    s.token_address,
                    "burn_public",
                    vec![
                        AbiValue::Field(Fr::from(s.admin_address)),
                        AbiValue::Integer(amount as i128),
                        AbiValue::Field(authwit_nonce),
                    ],
                )],
                ..Default::default()
            },
            SimulateOptions {
                from: s.account1_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("missing approval should fail");
    assert!(err.to_string().to_lowercase().contains("unauthorized"));
}

/// TS: public > burn more than balance on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_burn_more_than_balance_on_behalf_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance0 + 1;
    let authwit_nonce = Fr::random();
    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "burn_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let intent = MessageHashOrIntent::Intent {
        caller: s.account1_address,
        call: action.clone(),
    };
    let set_authwit =
        SetPublicAuthWitInteraction::create(&s.admin_wallet, s.admin_address, intent, true)
            .await
            .expect("create public authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send public authwit");

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
        .expect_err("underflow should fail");
    assert!(err.to_string().contains(U128_UNDERFLOW_ERROR));
}

/// TS: public > burn on behalf of other, wrong designated caller
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_burn_wrong_designated_caller_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = public_balance(&s.admin_wallet, s.token_address, &s.admin_address).await;
    let amount = balance0 + 2;
    let authwit_nonce = Fr::random();
    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "burn_public",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let wrong_intent = MessageHashOrIntent::Intent {
        caller: s.admin_address,
        call: action.clone(),
    };
    let set_authwit =
        SetPublicAuthWitInteraction::create(&s.admin_wallet, s.admin_address, wrong_intent, true)
            .await
            .expect("create wrong public authwit");
    set_authwit
        .send(SendOptions::default())
        .await
        .expect("send wrong public authwit");

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
        .expect_err("wrong caller should fail");
    assert!(err.to_string().to_lowercase().contains("unauthorized"));
}

// ---------------------------------------------------------------------------
// Tests: Private burn
// ---------------------------------------------------------------------------

/// TS: private > burn less than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_burn_less_than_balance() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    assert!(amount > 0);

    send_token_method(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "burn_private",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Integer(0),
        ],
        s.admin_address,
    )
    .await;

    let balance_after = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    assert_eq!(balance_after, balance0 - amount);
}

/// TS: private > burn on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_burn_on_behalf_of_other() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    let authwit_nonce = Fr::random();
    assert!(amount > 0);

    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "burn_private",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
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
        .expect("burn through authwit proxy");

    let balance_after = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    assert_eq!(balance_after, balance0 - amount);

    let replay_err = s
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
        .expect_err("duplicate nullifier replay should fail");
    assert!(replay_err
        .to_string()
        .to_lowercase()
        .contains(DUPLICATE_NULLIFIER_ERROR));
}

/// TS: private > burn more than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_burn_more_than_balance_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let err = s
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &s.token_artifact,
                    s.token_address,
                    "burn_private",
                    vec![
                        AbiValue::Field(Fr::from(s.admin_address)),
                        AbiValue::Integer((balance0 + 1) as i128),
                        AbiValue::Integer(0),
                    ],
                )],
                ..Default::default()
            },
            SimulateOptions {
                from: s.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("burn more than balance should fail");
    assert!(err.to_string().contains("Balance too low"));
}

/// TS: private > burn on behalf of self with non-zero nonce
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_burn_self_nonzero_nonce_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let err = s
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &s.token_artifact,
                    s.token_address,
                    "burn_private",
                    vec![
                        AbiValue::Field(Fr::from(s.admin_address)),
                        AbiValue::Integer((balance0 - 1) as i128),
                        AbiValue::Integer(1),
                    ],
                )],
                ..Default::default()
            },
            SimulateOptions {
                from: s.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("non-zero nonce on self burn should fail");
    assert!(err.to_string().contains("Invalid authwit nonce"));
}

/// TS: private > burn more than balance on behalf of other
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_burn_more_than_balance_on_behalf_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = balance0 + 1;
    let authwit_nonce = Fr::random();
    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "burn_private",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let witness = s
        .admin_wallet
        .create_auth_wit(
            s.admin_address,
            MessageHashOrIntent::Intent {
                caller: s.proxy_address,
                call: action.clone(),
            },
        )
        .await
        .expect("create authwit");

    let err = s
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_proxy_call(
                    &s.proxy_artifact,
                    s.proxy_address,
                    &action,
                )],
                auth_witnesses: vec![witness],
                ..Default::default()
            },
            SimulateOptions {
                from: s.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("balance too low on behalf should fail");
    assert!(err.to_string().contains("Balance too low"));
}

/// TS: private > burn on behalf of other without approval
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_burn_without_approval_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    let authwit_nonce = Fr::random();
    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "burn_private",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let message_hash = compute_auth_wit_message_hash(
        &MessageHashOrIntent::Intent {
            caller: s.proxy_address,
            call: action.clone(),
        },
        &s.admin_wallet.get_chain_info().await.expect("chain info"),
    );

    let err = s
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_proxy_call(
                    &s.proxy_artifact,
                    s.proxy_address,
                    &action,
                )],
                ..Default::default()
            },
            SimulateOptions {
                from: s.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("missing authwit should fail");
    assert!(
        err.to_string().contains(&format!(
            "Unknown auth witness for message hash {message_hash}"
        )) || err.to_string().contains("Unknown auth witness")
    );
}

/// TS: private > on behalf of other (invalid designated caller)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_burn_invalid_designated_caller_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = balance0 + 2;
    let authwit_nonce = Fr::random();
    let action = build_call(
        &s.token_artifact,
        s.token_address,
        "burn_private",
        vec![
            AbiValue::Field(Fr::from(s.admin_address)),
            AbiValue::Integer(amount as i128),
            AbiValue::Field(authwit_nonce),
        ],
    );
    let expected_hash = compute_auth_wit_message_hash(
        &MessageHashOrIntent::Intent {
            caller: s.proxy_address,
            call: action.clone(),
        },
        &s.admin_wallet.get_chain_info().await.expect("chain info"),
    );
    let witness = s
        .admin_wallet
        .create_auth_wit(
            s.admin_address,
            MessageHashOrIntent::Intent {
                caller: s.account1_address,
                call: action.clone(),
            },
        )
        .await
        .expect("create mismatched authwit");

    let err = s
        .admin_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![build_proxy_call(
                    &s.proxy_artifact,
                    s.proxy_address,
                    &action,
                )],
                auth_witnesses: vec![witness],
                ..Default::default()
            },
            SimulateOptions {
                from: s.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("wrong designated caller should fail");
    assert!(
        err.to_string().contains(&format!(
            "Unknown auth witness for message hash {expected_hash}"
        )) || err.to_string().contains("Unknown auth witness")
    );
}
