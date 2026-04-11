//! Partial notes tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_partial_notes.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_partial_notes -- --ignored --nocapture
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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionType};
use aztec_rs::account::{SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::tx::{ExecutionPayload, FunctionCall};
use aztec_rs::types::{AztecAddress, CompleteAddress, ContractInstanceWithAddress, Fr};
use aztec_rs::wallet::{BaseWallet, ExecuteUtilityOptions, SendOptions, Wallet};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_compiled_token_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/token_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse token_contract_compiled.json")
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

const INITIAL_TOKEN_BALANCE: u64 = 1_000_000_000;

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

    let primary_secret_key = Fr::from_hex(primary.secret_key).expect("valid secret key");
    let primary_complete = imported_complete_address(primary);
    pxe.key_store()
        .add_account(&primary_secret_key)
        .await
        .expect("seed primary key");
    pxe.address_store()
        .add(&primary_complete)
        .await
        .expect("seed primary address");

    for account in extra {
        let secret_key = Fr::from_hex(account.secret_key).expect("valid secret key");
        let complete = imported_complete_address(*account);
        pxe.key_store()
            .add_account(&secret_key)
            .await
            .expect("seed extra key");
        pxe.address_store()
            .add(&complete)
            .await
            .expect("seed extra address");
    }

    let provider = SingleAccountProvider::new(
        primary_complete.clone(),
        Box::new(SchnorrAccountContract::new(primary_secret_key)),
        primary.alias,
    );
    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, primary_complete.address))
}

async fn deploy_token(
    wallet: &TestWallet,
    admin: AztecAddress,
) -> (AztecAddress, ContractArtifact, ContractInstanceWithAddress) {
    let artifact = load_compiled_token_artifact();
    let result = Contract::deploy(
        wallet,
        artifact.clone(),
        vec![
            AbiValue::Field(Fr::from(admin)),
            AbiValue::String("TokenName".to_owned()),
            AbiValue::String("TokenSymbol".to_owned()),
            AbiValue::Integer(18),
        ],
        None,
    )
    .expect("token deploy builder")
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

    (result.instance.address, artifact, result.instance)
}

fn make_call(
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    function_type: Option<FunctionType>,
) -> FunctionCall {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|e| panic!("function '{method_name}' not found: {e}"));
    FunctionCall {
        to: contract_address,
        selector: func.selector.expect("selector"),
        args,
        function_type: function_type.unwrap_or_else(|| func.function_type.clone()),
        is_static: func.is_static,
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
    let call = make_call(artifact, token_address, method_name, args, None);
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
        .expect("send token method");
}

async fn call_utility_u128(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    token_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    scope: AztecAddress,
) -> u128 {
    let call = make_call(
        artifact,
        token_address,
        method_name,
        args,
        Some(FunctionType::Utility),
    );
    let result = wallet
        .execute_utility(
            call,
            ExecuteUtilityOptions {
                scope,
                ..Default::default()
            },
        )
        .await
        .expect("execute utility");

    result
        .result
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .and_then(|s| Fr::from_hex(s).ok())
        .map(|f| f.to_usize() as u128)
        .expect("utility result as u128")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// TS: mint to private
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn mint_to_private() {
    let _guard = serial_guard();

    let Some((wallet, admin_address)) = create_wallet(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1]).await
    else {
        return;
    };
    let liquidity_provider_address =
        AztecAddress(Fr::from_hex(TEST_ACCOUNT_1.address).expect("recipient address"));

    let (token_address, token_artifact, _) = deploy_token(&wallet, admin_address).await;

    send_token_method(
        &wallet,
        &token_artifact,
        token_address,
        "mint_to_private",
        vec![
            AbiValue::Field(Fr::from(liquidity_provider_address)),
            AbiValue::Integer(i128::from(INITIAL_TOKEN_BALANCE)),
        ],
        admin_address,
    )
    .await;

    let balance = call_utility_u128(
        &wallet,
        &token_artifact,
        token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(liquidity_provider_address))],
        liquidity_provider_address,
    )
    .await;

    assert_eq!(balance, u128::from(INITIAL_TOKEN_BALANCE));
}
