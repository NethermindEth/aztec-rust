//! State variable tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_state_vars.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_state_vars -- --ignored --nocapture
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

use aztec_rs::abi::{decode_from_abi, AbiDecoded, AbiValue, ContractArtifact, FunctionType};
use aztec_rs::account::{SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::{BatchCall, Contract};
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::tx::{ExecutionPayload, FunctionCall};
use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
use aztec_rs::wallet::{BaseWallet, ExecuteUtilityOptions, ProfileOptions, SendOptions, Wallet};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_state_vars_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/state_vars_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse state_vars_contract_compiled.json")
}

fn load_auth_contract_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/auth_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse auth_contract_compiled.json")
}

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

const VALUE: u64 = 2;
const RANDOMNESS: u64 = 2;
const AZTEC_SLOT_DURATION: u64 = 72;

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
    let wallet = Arc::new(BaseWallet::new(pxe, node, provider));
    Some((wallet, complete.address))
}

async fn deploy_contract(
    wallet: &TestWallet,
    artifact: ContractArtifact,
    constructor_args: Vec<AbiValue>,
    from: AztecAddress,
) -> AztecAddress {
    Contract::deploy(wallet, artifact, constructor_args, None)
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
    address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    function_type: Option<FunctionType>,
) -> FunctionCall {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|e| panic!("function '{method_name}' not found: {e}"));
    FunctionCall {
        to: address,
        selector: func.selector.expect("selector"),
        args,
        function_type: function_type.unwrap_or_else(|| func.function_type.clone()),
        is_static: func.is_static,
        hide_msg_sender: false,
    }
}

async fn call_utility(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    scope: AztecAddress,
) -> serde_json::Value {
    let raw = wallet
        .execute_utility(
            build_call(
                artifact,
                address,
                method_name,
                args,
                Some(FunctionType::Utility),
            ),
            ExecuteUtilityOptions {
                scope,
                ..Default::default()
            },
        )
        .await
        .expect("execute utility")
        .result;

    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|e| panic!("function '{method_name}' not found: {e}"));
    let fields = parse_field_array(&raw);
    decoded_to_json(
        &decode_from_abi(&func.return_types, &fields)
            .unwrap_or_else(|e| panic!("decode utility result for '{method_name}': {e}")),
    )
}

fn first_return(value: &serde_json::Value) -> &serde_json::Value {
    value.pointer("/returnValues/0").unwrap_or(value)
}

fn parse_bool(value: &serde_json::Value) -> bool {
    if let Some(b) = value.as_bool() {
        return b;
    }
    if let Some(arr) = value.as_array() {
        return parse_bool(arr.first().expect("bool array element"));
    }
    if let Some(s) = value.as_str() {
        return s == "true" || s == "0x01" || s == "1";
    }
    panic!("unexpected bool format: {value:?}");
}

fn parse_fr(value: &serde_json::Value) -> Fr {
    if let Some(s) = value.as_str() {
        return if s.starts_with("0x") {
            Fr::from_hex(s).unwrap_or_else(|_| panic!("parse Fr from {s}"))
        } else {
            s.parse::<u128>()
                .map(Fr::from)
                .unwrap_or_else(|_| panic!("parse Fr from {s}"))
        };
    }
    if let Some(n) = value.as_u64() {
        return Fr::from(n);
    }
    if let Some(arr) = value.as_array() {
        return parse_fr(arr.first().expect("array first"));
    }
    panic!("unexpected Fr format: {value:?}");
}

fn parse_field_array(value: &serde_json::Value) -> Vec<Fr> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("expected utility result array, got: {value:?}"))
        .iter()
        .map(parse_fr)
        .collect()
}

fn decoded_to_json(value: &AbiDecoded) -> serde_json::Value {
    match value {
        AbiDecoded::Field(fr) => serde_json::to_value(fr).expect("serialize Fr"),
        AbiDecoded::Boolean(boolean) => serde_json::Value::Bool(*boolean),
        AbiDecoded::Integer(integer) => serde_json::json!(integer),
        AbiDecoded::Array(items) | AbiDecoded::Tuple(items) => {
            serde_json::Value::Array(items.iter().map(decoded_to_json).collect())
        }
        AbiDecoded::String(string) => serde_json::Value::String(string.clone()),
        AbiDecoded::Struct(fields) => serde_json::Value::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), decoded_to_json(value)))
                .collect(),
        ),
        AbiDecoded::Address(address) => serde_json::to_value(address).expect("serialize address"),
        AbiDecoded::None => serde_json::Value::Null,
    }
}

fn simulated_return_fields(value: &serde_json::Value) -> Vec<Fr> {
    if let Some(values) = value.get("values") {
        return parse_field_array(values);
    }
    if let Some(values) = value.get("returnValues") {
        return parse_field_array(values);
    }
    parse_field_array(value)
}

fn decode_simulated_method_return(
    artifact: &ContractArtifact,
    method_name: &str,
    value: &serde_json::Value,
) -> serde_json::Value {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|e| panic!("function '{method_name}' not found: {e}"));
    let fields = simulated_return_fields(value);
    decoded_to_json(
        &decode_from_abi(&func.return_types, &fields)
            .unwrap_or_else(|e| panic!("decode simulated result for '{method_name}': {e}")),
    )
}

fn parse_mock_struct(value: &serde_json::Value) -> (AztecAddress, u64) {
    let obj = value.as_object().expect("mock struct object");
    let account = obj
        .get("account")
        .map(parse_fr)
        .map(AztecAddress)
        .expect("account");
    let val = obj.get("value").map(parse_fr).expect("value").to_usize() as u64;
    (account, val)
}

fn parse_mock_struct_fields(fields: &[Fr]) -> (AztecAddress, u64) {
    assert!(
        fields.len() >= 2,
        "expected at least 2 fields for MockStruct, got {}",
        fields.len()
    );
    (AztecAddress(fields[0]), fields[1].to_usize() as u64)
}

fn parse_field_note_value(value: &serde_json::Value) -> u64 {
    value
        .as_object()
        .and_then(|obj| obj.get("value"))
        .map(parse_fr)
        .expect("field note value")
        .to_usize() as u64
}

fn tx_effect_array_len(tx_effect: &serde_json::Value, pointer: &str) -> usize {
    tx_effect
        .pointer(pointer)
        .and_then(|v| v.as_array())
        .expect("tx effect array")
        .len()
}

fn profile_expiration_timestamp(profile_data: &serde_json::Value) -> u64 {
    profile_data
        .pointer("/data/expirationTimestamp")
        .or_else(|| profile_data.pointer("/expirationTimestamp"))
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| {
            panic!("expirationTimestamp not found in profile data: {profile_data:?}")
        })
}

// ---------------------------------------------------------------------------
// Tests: PublicImmutable
// ---------------------------------------------------------------------------

/// TS: PublicImmutable > private read of uninitialized PublicImmutable should fail
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_immutable_private_read_uninitialized_fails() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact, wallet.clone());

    let err = contract
        .method("get_public_immutable_constrained_private", vec![])
        .expect("build call")
        .simulate(aztec_rs::wallet::SimulateOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect_err("uninitialized private read should fail");
    assert!(!err.to_string().is_empty());
}

/// TS: PublicImmutable > initialize and read PublicImmutable
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_immutable_initialize_and_read() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact.clone(), wallet.clone());

    contract
        .method("initialize_public_immutable", vec![AbiValue::Integer(1)])
        .expect("build initialize_public_immutable")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("initialize public immutable");

    let read = call_utility(
        &wallet,
        &artifact,
        contract_address,
        "get_public_immutable",
        vec![],
        default_account_address,
    )
    .await;
    let (account, value) = parse_mock_struct(first_return(&read));
    assert_eq!(account, default_account_address);
    assert_eq!(value, 1);
}

/// TS: PublicImmutable > private read of initialized PublicImmutable
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_immutable_private_read_initialized() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact.clone(), wallet.clone());

    contract
        .method("initialize_public_immutable", vec![AbiValue::Integer(1)])
        .expect("build initialize_public_immutable")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("initialize public immutable");

    let a = contract
        .method("get_public_immutable_constrained_private", vec![])
        .expect("build direct private read")
        .simulate(aztec_rs::wallet::SimulateOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("simulate direct private read");
    let b = contract
        .method("get_public_immutable_constrained_private_indirect", vec![])
        .expect("build indirect private read")
        .simulate(aztec_rs::wallet::SimulateOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("simulate indirect private read");
    let utility = call_utility(
        &wallet,
        &artifact,
        contract_address,
        "get_public_immutable",
        vec![],
        default_account_address,
    )
    .await;

    let a_val = parse_mock_struct_fields(&simulated_return_fields(
        a.return_values
            .get("returnValues")
            .unwrap_or(&a.return_values),
    ));
    let b_val = parse_mock_struct_fields(&simulated_return_fields(
        b.return_values
            .get("returnValues")
            .unwrap_or(&b.return_values),
    ));
    let c_val = parse_mock_struct(first_return(&utility));
    assert_eq!(a_val, c_val);
    assert_eq!(b_val.0, c_val.0);
    assert_eq!(b_val.1, c_val.1 + 1);

    contract
        .method(
            "match_public_immutable",
            vec![
                AbiValue::Field(Fr::from(c_val.0)),
                AbiValue::Integer(c_val.1 as i128),
            ],
        )
        .expect("build match_public_immutable")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("match public immutable");
}

/// TS: PublicImmutable > public read of PublicImmutable
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_immutable_public_read() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact.clone(), wallet.clone());

    contract
        .method("initialize_public_immutable", vec![AbiValue::Integer(1)])
        .expect("build initialize_public_immutable")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("initialize public immutable");

    let a = contract
        .method("get_public_immutable_constrained_public", vec![])
        .expect("build direct public read");
    let b = contract
        .method("get_public_immutable_constrained_public_indirect", vec![])
        .expect("build indirect public read");
    a.simulate(aztec_rs::wallet::SimulateOptions {
        from: default_account_address,
        ..Default::default()
    })
    .await
    .expect("simulate direct public read");
    b.simulate(aztec_rs::wallet::SimulateOptions {
        from: default_account_address,
        ..Default::default()
    })
    .await
    .expect("simulate indirect public read");
    let utility = call_utility(
        &wallet,
        &artifact,
        contract_address,
        "get_public_immutable",
        vec![],
        default_account_address,
    )
    .await;
    let c_val = parse_mock_struct(first_return(&utility));
    assert_eq!(c_val.0, default_account_address);
    assert_eq!(c_val.1, 1);

    contract
        .method(
            "match_public_immutable",
            vec![
                AbiValue::Field(Fr::from(c_val.0)),
                AbiValue::Integer(c_val.1 as i128),
            ],
        )
        .expect("build match_public_immutable")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("match public immutable");
}

/// TS: PublicImmutable > public multiread of PublicImmutable
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_immutable_public_multiread() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact.clone(), wallet.clone());

    contract
        .method("initialize_public_immutable", vec![AbiValue::Integer(1)])
        .expect("build initialize_public_immutable")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("initialize public immutable");

    contract
        .method("get_public_immutable_constrained_public_multiple", vec![])
        .expect("build multiread")
        .simulate(aztec_rs::wallet::SimulateOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("simulate multiread");
    let expected = parse_mock_struct(first_return(
        &call_utility(
            &wallet,
            &artifact,
            contract_address,
            "get_public_immutable",
            vec![],
            default_account_address,
        )
        .await,
    ));
    assert_eq!(expected.0, default_account_address);
    assert_eq!(expected.1, 1);
}

/// TS: PublicImmutable > initializing PublicImmutable the second time should fail
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_immutable_reinitialize_fails() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact, wallet.clone());

    contract
        .method("initialize_public_immutable", vec![AbiValue::Integer(1)])
        .expect("build initialize_public_immutable")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("initialize public immutable");

    let err = contract
        .method("initialize_public_immutable", vec![AbiValue::Integer(1)])
        .expect("build second initialize")
        .simulate(aztec_rs::wallet::SimulateOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect_err("reinitialize should fail");
    assert!(err.to_string().contains("duplicate nullifier"));
}

// ---------------------------------------------------------------------------
// Tests: PrivateMutable
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mutable_read_uninitialized_fails() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;

    let initialized = parse_bool(first_return(
        &call_utility(
            &wallet,
            &artifact,
            contract_address,
            "is_private_mutable_initialized",
            vec![AbiValue::Field(Fr::from(default_account_address))],
            default_account_address,
        )
        .await,
    ));
    assert!(!initialized);

    let err = wallet
        .execute_utility(
            build_call(
                &artifact,
                contract_address,
                "get_private_mutable",
                vec![AbiValue::Field(Fr::from(default_account_address))],
                Some(FunctionType::Utility),
            ),
            ExecuteUtilityOptions {
                scope: default_account_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("uninitialized private mutable read should fail");
    assert!(!err.to_string().is_empty());
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mutable_initialize() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact.clone(), wallet.clone());

    let tx_hash = contract
        .method(
            "initialize_private",
            vec![
                AbiValue::Field(Fr::from(RANDOMNESS)),
                AbiValue::Field(Fr::from(VALUE)),
            ],
        )
        .expect("build initialize_private")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("initialize private mutable")
        .tx_hash;

    let tx_effect = wallet
        .node()
        .get_tx_effect(&tx_hash)
        .await
        .expect("get tx effect")
        .expect("tx effect exists");
    assert_eq!(tx_effect_array_len(&tx_effect, "/data/nullifiers"), 2);

    let initialized = parse_bool(first_return(
        &call_utility(
            &wallet,
            &artifact,
            contract_address,
            "is_private_mutable_initialized",
            vec![AbiValue::Field(Fr::from(default_account_address))],
            default_account_address,
        )
        .await,
    ));
    assert!(initialized);
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mutable_reinitialize_fails() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact, wallet.clone());

    contract
        .method(
            "initialize_private",
            vec![
                AbiValue::Field(Fr::from(RANDOMNESS)),
                AbiValue::Field(Fr::from(VALUE)),
            ],
        )
        .expect("build initialize_private")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("initialize private mutable");

    let err = contract
        .method(
            "initialize_private",
            vec![
                AbiValue::Field(Fr::from(RANDOMNESS)),
                AbiValue::Field(Fr::from(VALUE)),
            ],
        )
        .expect("build initialize_private again")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect_err("reinitialize should fail");
    assert!(!err.to_string().is_empty());
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mutable_read_initialized() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact.clone(), wallet.clone());

    contract
        .method(
            "initialize_private",
            vec![
                AbiValue::Field(Fr::from(RANDOMNESS)),
                AbiValue::Field(Fr::from(VALUE)),
            ],
        )
        .expect("build initialize_private")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("initialize private mutable");

    let value = parse_field_note_value(first_return(
        &call_utility(
            &wallet,
            &artifact,
            contract_address,
            "get_private_mutable",
            vec![AbiValue::Field(Fr::from(default_account_address))],
            default_account_address,
        )
        .await,
    ));
    assert_eq!(value, VALUE);
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mutable_replace_same_value() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact.clone(), wallet.clone());

    contract
        .method(
            "initialize_private",
            vec![
                AbiValue::Field(Fr::from(RANDOMNESS)),
                AbiValue::Field(Fr::from(VALUE)),
            ],
        )
        .expect("build initialize_private")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("initialize private mutable");

    let note_before = parse_field_note_value(first_return(
        &call_utility(
            &wallet,
            &artifact,
            contract_address,
            "get_private_mutable",
            vec![AbiValue::Field(Fr::from(default_account_address))],
            default_account_address,
        )
        .await,
    ));

    let tx_hash = contract
        .method(
            "update_private_mutable",
            vec![
                AbiValue::Field(Fr::from(RANDOMNESS)),
                AbiValue::Field(Fr::from(VALUE)),
            ],
        )
        .expect("build update_private_mutable")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("replace same value")
        .tx_hash;
    let tx_effect = wallet
        .node()
        .get_tx_effect(&tx_hash)
        .await
        .expect("get tx effect")
        .expect("tx effect exists");
    assert_eq!(tx_effect_array_len(&tx_effect, "/data/noteHashes"), 1);
    assert_eq!(tx_effect_array_len(&tx_effect, "/data/nullifiers"), 2);

    let note_after = parse_field_note_value(first_return(
        &call_utility(
            &wallet,
            &artifact,
            contract_address,
            "get_private_mutable",
            vec![AbiValue::Field(Fr::from(default_account_address))],
            default_account_address,
        )
        .await,
    ));
    assert_eq!(note_before, note_after);
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mutable_replace_other_values() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact.clone(), wallet.clone());

    contract
        .method(
            "initialize_private",
            vec![
                AbiValue::Field(Fr::from(RANDOMNESS)),
                AbiValue::Field(Fr::from(VALUE)),
            ],
        )
        .expect("build initialize_private")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("initialize private mutable");

    let tx_hash = contract
        .method(
            "update_private_mutable",
            vec![
                AbiValue::Field(Fr::from(RANDOMNESS + 2)),
                AbiValue::Field(Fr::from(VALUE + 1)),
            ],
        )
        .expect("build update_private_mutable")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("replace other value")
        .tx_hash;
    let tx_effect = wallet
        .node()
        .get_tx_effect(&tx_hash)
        .await
        .expect("get tx effect")
        .expect("tx effect exists");
    assert_eq!(tx_effect_array_len(&tx_effect, "/data/noteHashes"), 1);
    assert_eq!(tx_effect_array_len(&tx_effect, "/data/nullifiers"), 2);

    let value = parse_field_note_value(first_return(
        &call_utility(
            &wallet,
            &artifact,
            contract_address,
            "get_private_mutable",
            vec![AbiValue::Field(Fr::from(default_account_address))],
            default_account_address,
        )
        .await,
    ));
    assert_eq!(value, VALUE + 1);
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mutable_replace_dependent_on_prior() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact.clone(), wallet.clone());

    contract
        .method(
            "initialize_private",
            vec![
                AbiValue::Field(Fr::from(RANDOMNESS)),
                AbiValue::Field(Fr::from(VALUE)),
            ],
        )
        .expect("build initialize_private")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("initialize private mutable");

    let note_before = parse_field_note_value(first_return(
        &call_utility(
            &wallet,
            &artifact,
            contract_address,
            "get_private_mutable",
            vec![AbiValue::Field(Fr::from(default_account_address))],
            default_account_address,
        )
        .await,
    ));

    let tx_hash = contract
        .method("increase_private_value", vec![])
        .expect("build increase_private_value")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("increase private value")
        .tx_hash;
    let tx_effect = wallet
        .node()
        .get_tx_effect(&tx_hash)
        .await
        .expect("get tx effect")
        .expect("tx effect exists");
    assert_eq!(tx_effect_array_len(&tx_effect, "/data/noteHashes"), 1);
    assert_eq!(tx_effect_array_len(&tx_effect, "/data/nullifiers"), 2);

    let value = parse_field_note_value(first_return(
        &call_utility(
            &wallet,
            &artifact,
            contract_address,
            "get_private_mutable",
            vec![AbiValue::Field(Fr::from(default_account_address))],
            default_account_address,
        )
        .await,
    ));
    assert_eq!(value, note_before + 1);
}

// ---------------------------------------------------------------------------
// Tests: PrivateImmutable
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_immutable_read_uninitialized_fails() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;

    let initialized = parse_bool(first_return(
        &call_utility(
            &wallet,
            &artifact,
            contract_address,
            "is_priv_imm_initialized",
            vec![AbiValue::Field(Fr::from(default_account_address))],
            default_account_address,
        )
        .await,
    ));
    assert!(!initialized);

    let err = wallet
        .execute_utility(
            build_call(
                &artifact,
                contract_address,
                "view_private_immutable",
                vec![AbiValue::Field(Fr::from(default_account_address))],
                Some(FunctionType::Utility),
            ),
            ExecuteUtilityOptions {
                scope: default_account_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("uninitialized private immutable read should fail");
    assert!(!err.to_string().is_empty());
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_immutable_initialize() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact.clone(), wallet.clone());

    let tx_hash = contract
        .method(
            "initialize_private_immutable",
            vec![
                AbiValue::Field(Fr::from(RANDOMNESS)),
                AbiValue::Field(Fr::from(VALUE)),
            ],
        )
        .expect("build initialize_private_immutable")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("initialize private immutable")
        .tx_hash;
    let tx_effect = wallet
        .node()
        .get_tx_effect(&tx_hash)
        .await
        .expect("get tx effect")
        .expect("tx effect exists");
    assert_eq!(tx_effect_array_len(&tx_effect, "/data/noteHashes"), 1);
    assert_eq!(tx_effect_array_len(&tx_effect, "/data/nullifiers"), 2);

    let initialized = parse_bool(first_return(
        &call_utility(
            &wallet,
            &artifact,
            contract_address,
            "is_priv_imm_initialized",
            vec![AbiValue::Field(Fr::from(default_account_address))],
            default_account_address,
        )
        .await,
    ));
    assert!(initialized);
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_immutable_reinitialize_fails() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact, wallet.clone());

    contract
        .method(
            "initialize_private_immutable",
            vec![
                AbiValue::Field(Fr::from(RANDOMNESS)),
                AbiValue::Field(Fr::from(VALUE)),
            ],
        )
        .expect("build initialize_private_immutable")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("initialize private immutable");

    let err = contract
        .method(
            "initialize_private_immutable",
            vec![
                AbiValue::Field(Fr::from(RANDOMNESS)),
                AbiValue::Field(Fr::from(VALUE)),
            ],
        )
        .expect("build initialize_private_immutable again")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect_err("reinitialize private immutable should fail");
    assert!(!err.to_string().is_empty());
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_immutable_read_initialized() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let artifact = load_state_vars_artifact();
    let contract_address =
        deploy_contract(&wallet, artifact.clone(), vec![], default_account_address).await;
    let contract = Contract::at(contract_address, artifact.clone(), wallet.clone());

    contract
        .method(
            "initialize_private_immutable",
            vec![
                AbiValue::Field(Fr::from(RANDOMNESS)),
                AbiValue::Field(Fr::from(VALUE)),
            ],
        )
        .expect("build initialize_private_immutable")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("initialize private immutable");

    let value = parse_field_note_value(first_return(
        &call_utility(
            &wallet,
            &artifact,
            contract_address,
            "view_private_immutable",
            vec![AbiValue::Field(Fr::from(default_account_address))],
            default_account_address,
        )
        .await,
    ));
    assert_eq!(value, VALUE);
}

// ---------------------------------------------------------------------------
// Tests: DelayedPublicMutable
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn delayed_public_mutable_sets_expiration_timestamp() {
    let _guard = serial_guard();
    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };
    let auth_artifact = load_auth_contract_artifact();
    let auth_address = deploy_contract(
        &wallet,
        auth_artifact.clone(),
        vec![AbiValue::Field(Fr::from(default_account_address))],
        default_account_address,
    )
    .await;
    let auth_contract = Contract::at(auth_address, auth_artifact, wallet.clone());

    assert_eq!(AZTEC_SLOT_DURATION, 72);
    let new_delay = AZTEC_SLOT_DURATION * 2;

    auth_contract
        .method(
            "set_authorized_delay",
            vec![AbiValue::Integer(new_delay as i128)],
        )
        .expect("build set_authorized_delay")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("set authorized delay");

    for _ in 0..4 {
        auth_contract
            .method("get_authorized", vec![])
            .expect("build get_authorized")
            .send(SendOptions {
                from: default_account_address,
                ..Default::default()
            })
            .await
            .expect("mine block through get_authorized");
    }

    let latest_header = wallet
        .node()
        .get_block_header(0)
        .await
        .expect("latest block header");
    let latest_timestamp = latest_header
        .pointer("/globalVariables/timestamp")
        .or_else(|| latest_header.pointer("/data/globalVariables/timestamp"))
        .map(parse_fr)
        .map(|fr| fr.to_usize() as u64)
        .expect("latest timestamp");
    let expected_modified_expiration_timestamp = latest_timestamp + new_delay - 1;

    let profile = auth_contract
        .method("get_authorized_in_private", vec![])
        .expect("build get_authorized_in_private")
        .profile(ProfileOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("profile get_authorized_in_private");

    assert_eq!(
        profile_expiration_timestamp(&profile.profile_data),
        expected_modified_expiration_timestamp
    );
}
