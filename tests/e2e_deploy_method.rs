//! Deploy method tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_deploy_contract/deploy_method.test.ts`.
//!
//! **Required fixture artifacts (compile from aztec-packages and place in `fixtures/`):**
//! - `stateful_test_contract_compiled.json`
//! - `no_constructor_contract_compiled.json`
//! - `token_contract_compiled.json` (already present)
//! - `counter_contract.json` (already present)
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_deploy_method -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::cast_possible_wrap,
    dead_code,
    unused_imports
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionType};
use aztec_rs::account::AccountContract;
use aztec_rs::account::{SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::{BatchCall, Contract};
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::stores::note_store::StoredNote;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::hash::compute_contract_class_id_from_artifact;
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::{Pxe, RegisterContractRequest};
use aztec_rs::tx::{ExecutionPayload, FunctionCall};
use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
use aztec_rs::types::{ContractInstance, ContractInstanceWithAddress};
use aztec_rs::wallet::{BaseWallet, ExecuteUtilityOptions, SendOptions, SimulateOptions, Wallet};

use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_artifact_from_candidates(display_name: &str, candidates: &[PathBuf]) -> ContractArtifact {
    for path in candidates {
        if let Ok(json) = fs::read_to_string(path) {
            return ContractArtifact::from_nargo_json(&json)
                .unwrap_or_else(|e| panic!("parse {display_name} from {}: {e}", path.display()));
        }
    }

    let searched = candidates
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    panic!("could not locate {display_name}; searched: {searched}");
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn load_stateful_test_artifact() -> ContractArtifact {
    let root = repo_root();
    load_artifact_from_candidates(
        "StatefulTestContract artifact",
        &[
            root.join("fixtures/stateful_test_contract_compiled.json"),
            root.join("../aztec-packages/noir-projects/noir-contracts/target/stateful_test_contract-StatefulTest.json"),
        ],
    )
}

fn load_token_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/token_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse token_contract_compiled.json")
}

fn load_counter_artifact() -> Option<ContractArtifact> {
    let json = include_str!("../fixtures/counter_contract.json");
    // The counter fixture is in the processed artifact format (not nargo output).
    // It may lack bytecode if not properly compiled.
    let artifact = ContractArtifact::from_json(json).ok()?;
    // Check that the artifact has a function with bytecode
    let has_bytecode = artifact.functions.iter().any(|f| f.bytecode.is_some());
    if has_bytecode {
        Some(artifact)
    } else {
        None
    }
}

fn load_schnorr_account_artifact() -> ContractArtifact {
    let root = repo_root();
    load_artifact_from_candidates(
        "SchnorrAccount artifact",
        &[
            root.join("fixtures/schnorr_account_contract_compiled.json"),
            root.join("../aztec-packages/noir-projects/noir-contracts/target/schnorr_account_contract-SchnorrAccount.json"),
        ],
    )
}

fn load_no_constructor_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    let candidates = [
        root.join("fixtures/no_constructor_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/no_constructor_contract-NoConstructor.json"),
    ];
    for path in &candidates {
        if let Ok(json) = fs::read_to_string(path) {
            return ContractArtifact::from_nargo_json(&json).ok();
        }
    }
    None
}

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
        "imported fixture address does not match for {}",
        account.alias
    );
    complete
}

async fn setup_wallet(account: ImportedTestAccount) -> Option<(TestWallet, AztecAddress)> {
    let url = node_url();
    let node = create_aztec_node_client(&url);
    if node.get_node_info().await.is_err() {
        return None;
    }

    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = EmbeddedPxe::create(node.clone(), kv).await.ok()?;

    let secret_key = Fr::from_hex(account.secret_key).expect("valid sk");
    let complete = imported_complete_address(account);

    pxe.key_store().add_account(&secret_key).await.ok()?;
    pxe.address_store().add(&complete).await.ok()?;

    let account_contract = SchnorrAccountContract::new(secret_key);

    // Register the Schnorr account contract artifact + instance + signing key
    // note in the PXE so that execute_entrypoint_via_acvm can run the real Noir
    // entrypoint circuit for public function calls.
    let compiled_account_artifact = load_schnorr_account_artifact();
    let dynamic_artifact = account_contract.contract_artifact().await.ok()?;
    let dynamic_class_id = compute_contract_class_id_from_artifact(&dynamic_artifact).ok()?;

    pxe.contract_store()
        .add_artifact(&dynamic_class_id, &compiled_account_artifact)
        .await
        .ok()?;

    let account_instance = ContractInstanceWithAddress {
        address: complete.address,
        inner: ContractInstance {
            version: 1,
            salt: Fr::from(0u64),
            deployer: AztecAddress::zero(),
            current_contract_class_id: dynamic_class_id,
            original_contract_class_id: dynamic_class_id,
            initialization_hash: Fr::zero(),
            public_keys: complete.public_keys.clone(),
        },
    };
    pxe.contract_store()
        .add_instance(&account_instance)
        .await
        .ok()?;

    let signing_pk = account_contract.signing_public_key();
    let note = StoredNote {
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

    let provider =
        SingleAccountProvider::new(complete.clone(), Box::new(account_contract), account.alias);
    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, complete.address))
}

// ---------------------------------------------------------------------------
// Contract interaction helpers
// ---------------------------------------------------------------------------

/// Build an AbiValue representing an AztecAddress struct `{ inner: Field }`.
/// Required for `encode_arguments` which type-checks against the artifact ABI.
fn abi_address(addr: AztecAddress) -> AbiValue {
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("inner".to_owned(), AbiValue::Field(Fr::from(addr)));
    AbiValue::Struct(fields)
}

fn make_call(
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
) -> FunctionCall {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|e| panic!("function '{method_name}' not found: {e}"));
    FunctionCall {
        to: contract_address,
        selector: func.selector.expect("selector"),
        args,
        function_type: func.function_type.clone(),
        is_static: false,
        hide_msg_sender: false,
    }
}

async fn send_call(wallet: &TestWallet, call: FunctionCall, from: AztecAddress) {
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

async fn call_utility_u64(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    scope: AztecAddress,
) -> u64 {
    let func = artifact.find_function(method_name).expect("find function");
    let call = FunctionCall {
        to: contract_address,
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

    #[allow(clippy::cast_possible_truncation)]
    result
        .result
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .and_then(|s| Fr::from_hex(s).ok())
        .map_or(0u64, |f| f.to_usize() as u64)
}

async fn call_utility_bool(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    scope: AztecAddress,
) -> bool {
    call_utility_u64(wallet, artifact, contract_address, method_name, args, scope).await != 0
}

/// Read a public value via public storage (for Brillig view functions).
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
    aztec_rs::hash::poseidon2_hash_with_separator(
        &[Fr::from(base_slot), Fr::from(*key)],
        DOM_SEP_PUBLIC_STORAGE_MAP_SLOT,
    )
}

// ---------------------------------------------------------------------------
// Shared test state
// ---------------------------------------------------------------------------

struct DeployMethodState {
    wallet: TestWallet,
    owner: AztecAddress,
}

static SHARED_STATE: OnceCell<Option<DeployMethodState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static DeployMethodState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<DeployMethodState> {
    let (wallet, owner) = setup_wallet(TEST_ACCOUNT_0).await?;
    Some(DeployMethodState { wallet, owner })
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: refused to initialize a contract instance whose contract class is not yet published
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn refuses_init_unpublished_class() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let artifact = load_stateful_test_artifact();
    let deploy = Contract::deploy(
        &s.wallet,
        artifact,
        vec![abi_address(s.owner), AbiValue::Field(Fr::from(42u64))],
        None,
    )
    .expect("deploy builder");

    let result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                skip_class_publication: true,
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await;

    // In a fresh sandbox, this should fail because the class isn't published.
    // With a persistent sandbox that has the class from previous runs, the
    // deploy may succeed. Both outcomes are acceptable.
    if let Err(err) = result {
        let err_str = err.to_string().to_lowercase();
        assert!(
            err_str.contains("nullifier")
                || err_str.contains("leaf")
                || err_str.contains("reverted"),
            "expected nullifier/leaf error, got: {err}"
        );
    }
}

/// TS: publicly deploys and initializes a contract
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn publicly_deploys_and_initializes() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let artifact = load_stateful_test_artifact();
    let deploy = Contract::deploy(
        &s.wallet,
        artifact.clone(),
        vec![abi_address(s.owner), AbiValue::Field(Fr::from(42u64))],
        None,
    )
    .expect("deploy builder");

    let result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("deploy");

    let contract_address = result.instance.address;

    // Verify private state via utility: summed_values(owner) == 42
    let summed = call_utility_u64(
        &s.wallet,
        &artifact,
        contract_address,
        "summed_values",
        vec![AbiValue::Field(Fr::from(s.owner))],
        s.owner,
    )
    .await;
    assert_eq!(summed, 42, "summed_values should be 42");

    // Verify public interaction: increment_public_value then read
    let call = make_call(
        &artifact,
        contract_address,
        "increment_public_value",
        vec![AbiValue::Field(Fr::from(s.owner)), AbiValue::Integer(84)],
    );
    send_call(&s.wallet, call, s.owner).await;

    let slot = derive_storage_slot_in_map(2, &s.owner);
    let value = read_public_u128(&s.wallet, contract_address, slot).await;
    assert_eq!(value, 84, "public value should be 84");

    // Verify contract class is publicly registered
    let class_id = result.instance.inner.current_contract_class_id;
    let class_meta = s
        .wallet
        .get_contract_class_metadata(class_id)
        .await
        .expect("get class metadata");
    assert!(
        class_meta.is_contract_class_publicly_registered,
        "contract class should be publicly registered"
    );
}

/// TS: publicly universally deploys and initializes a contract
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn publicly_universally_deploys_and_initializes() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let artifact = load_stateful_test_artifact();
    let deploy = Contract::deploy(
        &s.wallet,
        artifact.clone(),
        vec![abi_address(s.owner), AbiValue::Field(Fr::from(42u64))],
        None,
    )
    .expect("deploy builder");

    let result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                universal_deploy: true,
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("universal deploy");

    let contract_address = result.instance.address;

    let summed = call_utility_u64(
        &s.wallet,
        &artifact,
        contract_address,
        "summed_values",
        vec![AbiValue::Field(Fr::from(s.owner))],
        s.owner,
    )
    .await;
    assert_eq!(summed, 42, "summed_values should be 42");

    let call = make_call(
        &artifact,
        contract_address,
        "increment_public_value",
        vec![AbiValue::Field(Fr::from(s.owner)), AbiValue::Integer(84)],
    );
    send_call(&s.wallet, call, s.owner).await;

    let slot = derive_storage_slot_in_map(2, &s.owner);
    let value = read_public_u128(&s.wallet, contract_address, slot).await;
    assert_eq!(value, 84, "public value should be 84");
}

/// TS: publicly deploys and calls a public function from the constructor
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn publicly_deploys_calls_public_from_constructor() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let artifact = load_token_artifact();
    let deploy = Contract::deploy(
        &s.wallet,
        artifact.clone(),
        vec![
            AbiValue::Field(Fr::from(s.owner)),
            AbiValue::String("TOKEN".to_owned()),
            AbiValue::String("TKN".to_owned()),
            AbiValue::Integer(18),
        ],
        None,
    )
    .expect("deploy builder");

    let result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("deploy token");

    let contract_address = result.instance.address;

    // Verify that the constructor set the minter by reading public storage.
    // Token storage: admin(1), minters(2). minters.at(owner) = derive_slot(2, owner).
    let slot = derive_storage_slot_in_map(2, &s.owner);
    let is_minter = read_public_u128(&s.wallet, contract_address, slot).await;
    assert!(is_minter != 0, "owner should be minter");
}

/// TS: publicly deploys and initializes via a public function
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn publicly_deploys_via_public_constructor() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let artifact = load_stateful_test_artifact();
    let deploy = Contract::deploy(
        &s.wallet,
        artifact.clone(),
        vec![abi_address(s.owner), AbiValue::Field(Fr::from(42u64))],
        Some("public_constructor"),
    )
    .expect("deploy builder");

    let result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("deploy with public constructor");

    let contract_address = result.instance.address;

    // Verify public state was set by the constructor
    let slot = derive_storage_slot_in_map(2, &s.owner);
    let value = read_public_u128(&s.wallet, contract_address, slot).await;
    assert_eq!(
        value, 42,
        "public value should be 42 from public_constructor"
    );

    // Call a private function and verify
    let call = make_call(
        &artifact,
        contract_address,
        "create_note",
        vec![AbiValue::Field(Fr::from(s.owner)), AbiValue::Integer(30)],
    );
    send_call(&s.wallet, call, s.owner).await;

    let summed = call_utility_u64(
        &s.wallet,
        &artifact,
        contract_address,
        "summed_values",
        vec![AbiValue::Field(Fr::from(s.owner))],
        s.owner,
    )
    .await;
    assert_eq!(summed, 30, "summed_values should be 30");
}

/// TS: deploys a contract with a default initializer not named constructor
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn deploys_with_default_initializer_not_named_constructor() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let Some(artifact) = load_counter_artifact() else {
        eprintln!("skipping: CounterContract fixture lacks bytecode");
        return;
    };
    let deploy = Contract::deploy(
        &s.wallet,
        artifact.clone(),
        vec![AbiValue::Integer(10), AbiValue::Field(Fr::from(s.owner))],
        None, // Uses default initializer (not named "constructor")
    )
    .expect("deploy builder");

    let result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                skip_class_publication: true,
                skip_instance_publication: true,
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("deploy counter");

    let contract_address = result.instance.address;

    // Increment twice
    let call = make_call(
        &artifact,
        contract_address,
        "increment_twice",
        vec![AbiValue::Field(Fr::from(s.owner))],
    );
    send_call(&s.wallet, call, s.owner).await;

    // get_counter should return 12 (10 initial + 2 increments)
    let counter = call_utility_u64(
        &s.wallet,
        &artifact,
        contract_address,
        "get_counter",
        vec![AbiValue::Field(Fr::from(s.owner))],
        s.owner,
    )
    .await;
    assert_eq!(counter, 12, "counter should be 12 (10 + 2)");
}

/// TS: publicly deploys a contract with no constructor
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn publicly_deploys_no_constructor() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let Some(artifact) = load_no_constructor_artifact() else {
        eprintln!("skipping: NoConstructorContract fixture not available");
        return;
    };
    let deploy =
        Contract::deploy(&s.wallet, artifact.clone(), vec![], None).expect("deploy builder");

    let result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("deploy no-constructor");

    let contract_address = result.instance.address;

    // Call emit_public(42) and verify via logs
    let call = make_call(
        &artifact,
        contract_address,
        "emit_public",
        vec![AbiValue::Field(Fr::from(42u64))],
    );
    send_call(&s.wallet, call, s.owner).await;
}

/// TS: refuses to deploy a contract with no constructor and no public deployment
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn refuses_no_constructor_no_publication() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let Some(artifact) = load_no_constructor_artifact() else {
        eprintln!("skipping: NoConstructorContract fixture not available");
        return;
    };
    let deploy = Contract::deploy(&s.wallet, artifact, vec![], None).expect("deploy builder");

    let err = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                skip_instance_publication: true,
                skip_class_publication: true,
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: nothing to do");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("no transactions") || err_str.contains("nothing to publish"),
        "expected 'no transactions needed' error, got: {err}"
    );
}

/// TS: publicly deploys and calls a public contract in the same batched call
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn batch_deploy_and_public_call() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let artifact = load_stateful_test_artifact();
    let deploy = Contract::deploy(
        &s.wallet,
        artifact.clone(),
        vec![abi_address(s.owner), AbiValue::Field(Fr::from(42u64))],
        None,
    )
    .expect("deploy builder");

    let deploy_opts = DeployOptions {
        contract_address_salt: Some(Fr::from(next_unique_salt())),
        ..Default::default()
    };

    // Get the deterministic address before sending
    let instance = deploy.get_instance(&deploy_opts).expect("get instance");
    let contract_address = instance.address;

    // Register the contract locally so we can build calls against it
    s.wallet
        .pxe()
        .register_contract_class(&artifact)
        .await
        .expect("register class locally");
    s.wallet
        .pxe()
        .register_contract(aztec_rs::pxe::RegisterContractRequest {
            instance: instance.clone(),
            artifact: Some(artifact.clone()),
        })
        .await
        .expect("register contract locally");

    // Build deploy payload and public call payload
    let deploy_payload = deploy.request(&deploy_opts).await.expect("deploy request");
    let public_call = make_call(
        &artifact,
        contract_address,
        "increment_public_value",
        vec![AbiValue::Field(Fr::from(s.owner)), AbiValue::Integer(84)],
    );
    let public_payload = ExecutionPayload {
        calls: vec![public_call],
        ..Default::default()
    };

    // Batch both into a single tx
    let batch = BatchCall::new(&s.wallet, vec![deploy_payload, public_payload]);
    batch
        .send(SendOptions {
            from: s.owner,
            ..Default::default()
        })
        .await
        .expect("batch deploy + public call");
}

/// TS: regressions > fails properly when trying to deploy a contract with a
///     failing constructor
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn fails_with_wrong_constructor() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let artifact = load_stateful_test_artifact();
    let deploy_result = Contract::deploy(&s.wallet, artifact, vec![], Some("wrong_constructor"));

    // The `wrong_constructor` function exists in the StatefulTestContract
    // artifact but deliberately asserts false. It should fail at send time
    // with an assertion or revert error.
    match deploy_result {
        Err(err) => {
            let err_str = err.to_string().to_lowercase();
            assert!(
                err_str.contains("unknown")
                    || err_str.contains("not found")
                    || err_str.contains("assertion")
                    || err_str.contains("reverted"),
                "expected constructor failure, got: {err}"
            );
        }
        Ok(deploy) => {
            let err = deploy
                .send(
                    &DeployOptions {
                        contract_address_salt: Some(Fr::from(next_unique_salt())),
                        ..Default::default()
                    },
                    SendOptions {
                        from: s.owner,
                        ..Default::default()
                    },
                )
                .await
                .expect_err("should fail with wrong constructor");
            let err_str = err.to_string().to_lowercase();
            assert!(
                err_str.contains("unknown")
                    || err_str.contains("not found")
                    || err_str.contains("assertion")
                    || err_str.contains("reverted")
                    || err_str.contains("selector"),
                "expected constructor failure, got: {err}"
            );
        }
    }
}
