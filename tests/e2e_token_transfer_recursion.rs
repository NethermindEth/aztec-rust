//! Private transfer recursion tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/private_transfer_recursion.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_transfer_recursion -- --ignored --nocapture
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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{
    AbiParameter, AbiType, AbiValue, ContractArtifact, EventSelector, FunctionType,
};
use aztec_rs::account::{SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::Pxe;
use aztec_rs::tx::{ExecutionPayload, FunctionCall, TxHash};
use aztec_rs::types::{AztecAddress, CompleteAddress, ContractInstanceWithAddress, Fr};
use aztec_rs::wallet::{
    BaseWallet, EventMetadataDefinition, ExecuteUtilityOptions, PrivateEventFilter, SendOptions,
    SimulateOptions, Wallet,
};

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
        pxe.register_sender(&complete.address)
            .await
            .expect("register sender");
    }

    let provider = SingleAccountProvider::new(
        primary_complete.clone(),
        Box::new(SchnorrAccountContract::new(primary_secret_key)),
        primary.alias,
    );
    let wallet = Arc::new(BaseWallet::new(pxe, node, provider));
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
            AbiValue::String("USDC".to_owned()),
            AbiValue::String("USD".to_owned()),
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
) -> TxHash {
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
        .expect("send token method")
        .tx_hash
}

async fn simulate_token_method(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    token_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    from: AztecAddress,
) -> Result<(), aztec_rs::Error> {
    let call = make_call(artifact, token_address, method_name, args, None);
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

async fn mint_notes(
    wallet: &TestWallet,
    minter: AztecAddress,
    recipient: AztecAddress,
    token_address: AztecAddress,
    artifact: &ContractArtifact,
    note_amounts: &[u64],
) -> u128 {
    let mut total = 0u128;
    for batch in note_amounts.chunks(5) {
        let calls: Vec<FunctionCall> = batch
            .iter()
            .map(|amount| {
                total += u128::from(*amount);
                make_call(
                    artifact,
                    token_address,
                    "mint_to_private",
                    vec![
                        AbiValue::Field(Fr::from(recipient)),
                        AbiValue::Integer(i128::from(*amount)),
                    ],
                    None,
                )
            })
            .collect();
        wallet
            .send_tx(
                ExecutionPayload {
                    calls,
                    ..Default::default()
                },
                SendOptions {
                    from: minter,
                    ..Default::default()
                },
            )
            .await
            .expect("batch mint notes");
    }
    total
}

fn tx_effect_array_len(tx_effect: &serde_json::Value, pointer: &str) -> usize {
    tx_effect
        .pointer(pointer)
        .and_then(|v| v.as_array())
        .map_or_else(|| panic!("missing tx effect field {pointer}"), Vec::len)
}

fn event_selector_from_signature(signature: &str) -> EventSelector {
    EventSelector(aztec_rs::abi::FunctionSelector::from_signature(signature).to_field())
}

fn transfer_event_metadata() -> EventMetadataDefinition {
    EventMetadataDefinition {
        event_selector: event_selector_from_signature("Transfer((Field),(Field),u128)"),
        abi_type: AbiType::Struct {
            name: "Transfer".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "from".to_owned(),
                    typ: AbiType::Struct {
                        name: "AztecAddress".to_owned(),
                        fields: vec![AbiParameter {
                            name: "inner".to_owned(),
                            typ: AbiType::Field,
                            visibility: None,
                        }],
                    },
                    visibility: None,
                },
                AbiParameter {
                    name: "to".to_owned(),
                    typ: AbiType::Struct {
                        name: "AztecAddress".to_owned(),
                        fields: vec![AbiParameter {
                            name: "inner".to_owned(),
                            typ: AbiType::Field,
                            visibility: None,
                        }],
                    },
                    visibility: None,
                },
                AbiParameter {
                    name: "amount".to_owned(),
                    typ: AbiType::Integer {
                        sign: "unsigned".to_owned(),
                        width: 128,
                    },
                    visibility: None,
                },
            ],
        },
        field_names: vec!["from".to_owned(), "to".to_owned(), "amount".to_owned()],
    }
}

fn parse_event_field_as_fr(event: &serde_json::Value, key: &str) -> Fr {
    event
        .get(key)
        .and_then(|v| v.as_str())
        .and_then(|s| Fr::from_hex(s).ok())
        .unwrap_or_else(|| panic!("parse event field {key}"))
}

// ---------------------------------------------------------------------------
// Tests: Private transfer recursion
// ---------------------------------------------------------------------------

/// TS: transfer full balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_full_balance() {
    let _guard = serial_guard();

    let Some((wallet, admin_address)) = create_wallet(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1]).await
    else {
        return;
    };
    let account1_address =
        AztecAddress(Fr::from_hex(TEST_ACCOUNT_1.address).expect("account1 address"));

    let (token_address, token_artifact, _) = deploy_token(&wallet, admin_address).await;

    let total_notes = 16usize;
    let total_balance = mint_notes(
        &wallet,
        admin_address,
        admin_address,
        token_address,
        &token_artifact,
        &vec![10u64; total_notes],
    )
    .await;

    let tx_hash = send_token_method(
        &wallet,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(account1_address)),
            AbiValue::Integer(total_balance as i128),
        ],
        admin_address,
    )
    .await;

    let receipt = wallet
        .node()
        .get_tx_receipt(&tx_hash)
        .await
        .expect("get tx receipt");
    let tx_effect = wallet
        .node()
        .get_tx_effect(&tx_hash)
        .await
        .expect("get tx effect")
        .expect("tx effect exists");

    assert_eq!(
        tx_effect_array_len(&tx_effect, "/data/nullifiers"),
        total_notes + 1 + 1
    );
    assert_eq!(tx_effect_array_len(&tx_effect, "/data/noteHashes"), 1);

    let events = wallet
        .get_private_events(
            &transfer_event_metadata(),
            PrivateEventFilter {
                contract_address: token_address,
                from_block: receipt.block_number,
                to_block: receipt.block_number.map(|n| n + 1),
                scopes: vec![account1_address],
                ..Default::default()
            },
        )
        .await
        .expect("get private transfer events");

    let event = &events[0];
    assert_eq!(
        parse_event_field_as_fr(&event.event, "from"),
        Fr::from(admin_address)
    );
    assert_eq!(
        parse_event_field_as_fr(&event.event, "to"),
        Fr::from(account1_address)
    );
    assert_eq!(
        parse_event_field_as_fr(&event.event, "amount"),
        Fr::from(u64::try_from(total_balance).expect("total balance fits in u64"))
    );
    assert_eq!(event.metadata.tx_hash, tx_hash);
    assert_eq!(event.metadata.block_number, receipt.block_number);
}

/// TS: transfer less than full balance and get change
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_less_than_full_balance_with_change() {
    let _guard = serial_guard();

    let Some((wallet, admin_address)) = create_wallet(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1]).await
    else {
        return;
    };
    let account1_address =
        AztecAddress(Fr::from_hex(TEST_ACCOUNT_1.address).expect("account1 address"));

    let (token_address, token_artifact, _) = deploy_token(&wallet, admin_address).await;

    let note_amounts = [10u64, 10, 10, 10];
    let expected_change = 3u128;
    let total_balance = mint_notes(
        &wallet,
        admin_address,
        admin_address,
        token_address,
        &token_artifact,
        &note_amounts,
    )
    .await;
    let to_send = total_balance - expected_change;

    let tx_hash = send_token_method(
        &wallet,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(account1_address)),
            AbiValue::Integer(to_send as i128),
        ],
        admin_address,
    )
    .await;

    let receipt = wallet
        .node()
        .get_tx_receipt(&tx_hash)
        .await
        .expect("get tx receipt");
    let tx_effect = wallet
        .node()
        .get_tx_effect(&tx_hash)
        .await
        .expect("get tx effect")
        .expect("tx effect exists");

    assert_eq!(
        tx_effect_array_len(&tx_effect, "/data/nullifiers"),
        note_amounts.len() + 1 + 1
    );
    assert_eq!(tx_effect_array_len(&tx_effect, "/data/noteHashes"), 2);

    let sender_balance = call_utility_u128(
        &wallet,
        &token_artifact,
        token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(admin_address))],
        admin_address,
    )
    .await;
    assert_eq!(sender_balance, expected_change);

    let events = wallet
        .get_private_events(
            &transfer_event_metadata(),
            PrivateEventFilter {
                contract_address: token_address,
                from_block: receipt.block_number,
                to_block: receipt.block_number.map(|n| n + 1),
                scopes: vec![account1_address],
                ..Default::default()
            },
        )
        .await
        .expect("get private transfer events");

    let event = &events[0];
    assert_eq!(
        parse_event_field_as_fr(&event.event, "from"),
        Fr::from(admin_address)
    );
    assert_eq!(
        parse_event_field_as_fr(&event.event, "to"),
        Fr::from(account1_address)
    );
    assert_eq!(
        parse_event_field_as_fr(&event.event, "amount"),
        Fr::from(u64::try_from(to_send).expect("transfer amount fits in u64"))
    );
    assert_eq!(event.metadata.tx_hash, tx_hash);
    assert_eq!(event.metadata.block_number, receipt.block_number);
}

/// TS: transfer more than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_more_than_balance_fails() {
    let _guard = serial_guard();

    let Some((wallet, admin_address)) = create_wallet(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1]).await
    else {
        return;
    };
    let account1_address =
        AztecAddress(Fr::from_hex(TEST_ACCOUNT_1.address).expect("account1 address"));

    let (token_address, token_artifact, _) = deploy_token(&wallet, admin_address).await;

    let balance = call_utility_u128(
        &wallet,
        &token_artifact,
        token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(admin_address))],
        admin_address,
    )
    .await;
    let amount = balance + 1;
    assert!(amount > 0);

    let err = simulate_token_method(
        &wallet,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(account1_address)),
            AbiValue::Integer(amount as i128),
        ],
        admin_address,
    )
    .await
    .expect_err("transfer more than balance should fail");

    assert!(
        err.to_string().contains("Balance too low"),
        "expected Balance too low, got: {err}"
    );
}
