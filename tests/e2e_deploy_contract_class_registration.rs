//! Contract class registration tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_deploy_contract/contract_class_registration.test.ts`.
//!
//! Artifact loading behavior:
//! - prefer `fixtures/stateful_test_contract_compiled.json`
//! - otherwise fall back to the canonical upstream compile output at
//!   `../aztec-packages/noir-projects/noir-contracts/target/stateful_test_contract-StatefulTest.json`
//! - prefer `fixtures/test_contract_compiled.json`
//! - otherwise fall back to
//!   `../aztec-packages/noir-projects/noir-contracts/target/test_contract-Test.json`
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_deploy_contract_class_registration -- --ignored --nocapture
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

use aztec_core::grumpkin;
use aztec_rs::abi::{encode_arguments, AbiValue, ContractArtifact, FunctionSelector, FunctionType};
use aztec_rs::account::{AccountContract, SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::{BatchCall, Contract, ContractFunctionInteraction};
use aztec_rs::crypto::{complete_address_from_secret_key_and_partial_address, derive_keys};
use aztec_rs::deployment::{
    get_contract_instance_from_instantiation_params, publish_contract_class, publish_instance,
    ContractInstantiationParams, DeployOptions,
};
use aztec_rs::embedded_pxe::stores::note_store::StoredNote;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::hash::{compute_contract_class_id_from_artifact, silo_nullifier};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::{Pxe, RegisterContractRequest};
use aztec_rs::tx::{ExecutionPayload, FunctionCall, TxExecutionResult, TxReceipt};
use aztec_rs::types::{
    AztecAddress, CompleteAddress, ContractInstance, ContractInstanceWithAddress, Fr, PublicKeys,
};
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

fn load_test_contract_artifact() -> ContractArtifact {
    let root = repo_root();
    load_artifact_from_candidates(
        "TestContract artifact",
        &[
            root.join("fixtures/test_contract_compiled.json"),
            root.join(
                "../aztec-packages/noir-projects/noir-contracts/target/test_contract-Test.json",
            ),
        ],
    )
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

// ---------------------------------------------------------------------------
// Constants (mirrors upstream fixtures/fixtures.ts)
// ---------------------------------------------------------------------------

/// Upstream: `DUPLICATE_NULLIFIER_ERROR = /dropped|nullifier|reverted/i`
const DUPLICATE_NULLIFIER_ERROR: &[&str] = &["dropped", "nullifier", "reverted"];

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
        "imported fixture address does not match derived complete address for {}",
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

    // Register the Schnorr account contract artifact + instance in the PXE
    // so that execute_entrypoint_via_acvm can run the real Noir entrypoint
    // circuit for public function calls.
    //
    // The pre-imported accounts were deployed with the dynamic artifact's
    // class ID (from SchnorrAccountContract). We compute that class ID and
    // store the compiled artifact (with bytecode) under it, then register
    // the contract instance so the PXE can look it up by address.
    let compiled_account_artifact = load_schnorr_account_artifact();
    let dynamic_artifact = account_contract.contract_artifact().await.ok()?;
    let dynamic_class_id =
        aztec_rs::hash::compute_contract_class_id_from_artifact(&dynamic_artifact).ok()?;

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

    // Seed the signing public key note into the PXE's note store.
    // Pre-imported accounts were deployed at genesis; their notes aren't
    // discoverable through standard sync. We insert the signing key note
    // directly so that execute_entrypoint_via_acvm can verify the Schnorr
    // signature in the account entrypoint circuit.
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

fn abi_address(address: AztecAddress) -> AbiValue {
    AbiValue::Field(Fr::from(address))
}

fn abi_field(value: u64) -> AbiValue {
    AbiValue::Field(Fr::from(value))
}

fn default_initializer_name(artifact: &ContractArtifact) -> Option<String> {
    let initializers: Vec<_> = artifact
        .functions
        .iter()
        .filter(|func| func.is_initializer)
        .collect();

    match initializers.as_slice() {
        [] => None,
        [func] => Some(func.name.clone()),
        funcs => funcs
            .iter()
            .find(|func| func.name == "constructor")
            .or_else(|| funcs.iter().find(|func| func.name == "initializer"))
            .or_else(|| funcs.iter().find(|func| func.parameters.is_empty()))
            .or_else(|| {
                funcs
                    .iter()
                    .find(|func| matches!(func.function_type, FunctionType::Private))
            })
            .or_else(|| funcs.first())
            .map(|func| func.name.clone()),
    }
}

fn stateful_ctor_args(owner: AztecAddress, value: u64) -> Vec<AbiValue> {
    vec![abi_address(owner), abi_field(value)]
}

fn random_valid_address() -> AztecAddress {
    loop {
        let candidate = Fr::random();
        if grumpkin::point_from_x(candidate).is_ok() {
            return AztecAddress(candidate);
        }
    }
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
        is_static: func.is_static,
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

async fn send_call_should_fail(
    wallet: &TestWallet,
    call: FunctionCall,
    from: AztecAddress,
    expected_fragments: &[&str],
) {
    let err = wallet
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
        .expect_err("should fail");

    let err_str = err.to_string().to_lowercase();
    let matches = expected_fragments
        .iter()
        .any(|frag| err_str.contains(&frag.to_lowercase()));
    assert!(
        matches,
        "expected one of {:?}, got: {}",
        expected_fragments, err
    );
}

async fn simulate_should_fail(
    wallet: &TestWallet,
    call: FunctionCall,
    from: AztecAddress,
    expected_fragments: &[&str],
) {
    let err = wallet
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
        .expect_err("should fail");

    let err_str = err.to_string().to_lowercase();
    let matches = expected_fragments
        .iter()
        .any(|frag| err_str.contains(&frag.to_lowercase()));
    assert!(
        matches,
        "expected one of {:?}, got: {}",
        expected_fragments, err
    );
}

/// Read a public value from the `public_values` map on the
/// `StatefulTestContract` by computing the map storage slot and reading
/// directly from the node's public storage.
///
/// The `public_values` map lives at base slot 1 (the first storage field
/// in `StatefulTestContract`). The derived slot for a key `whom` is
/// `poseidon2_hash_with_separator([base_slot, whom], MAP_SLOT_DERIV_SEP)`.
async fn read_public_value(
    wallet: &TestWallet,
    _artifact: &ContractArtifact,
    contract_address: AztecAddress,
    whom: AztecAddress,
    _scope: AztecAddress,
) -> u64 {
    // StatefulTestContract: storage { notes: PrivateSet, public_values: Map<...> }
    // base_slot = 2 (second storage field), MAP_SLOT_DERIV_SEPARATOR = 4015149901
    let base_slot = Fr::from(2u64);
    let slot = aztec_rs::hash::poseidon2_hash_with_separator(
        &[base_slot, Fr::from(whom)],
        4_015_149_901, // DOM_SEP__PUBLIC_STORAGE_MAP_SLOT
    );
    let value = wallet
        .pxe()
        .node()
        .get_public_storage_at(0, &contract_address, &slot)
        .await
        .expect("read public storage");
    value.to_usize() as u64
}

/// Helper: create an instance from instantiation params, publish it, and
/// register it locally. Mirrors the upstream `publishInstance` local helper.
async fn create_and_publish_instance(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    init_args: Vec<AbiValue>,
    constructor_name: Option<&str>,
    from: AztecAddress,
) -> (ContractInstanceWithAddress, Vec<AbiValue>) {
    let salt = Fr::from(next_unique_salt());
    let public_keys = PublicKeys {
        master_nullifier_public_key: grumpkin::scalar_mul(
            &aztec_rs::types::Fq::random(),
            &grumpkin::generator(),
        ),
        master_incoming_viewing_public_key: grumpkin::scalar_mul(
            &aztec_rs::types::Fq::random(),
            &grumpkin::generator(),
        ),
        master_outgoing_viewing_public_key: grumpkin::scalar_mul(
            &aztec_rs::types::Fq::random(),
            &grumpkin::generator(),
        ),
        master_tagging_public_key: grumpkin::scalar_mul(
            &aztec_rs::types::Fq::random(),
            &grumpkin::generator(),
        ),
    };
    let effective_constructor_name = constructor_name
        .map(str::to_owned)
        .or_else(|| default_initializer_name(artifact));

    let instance = get_contract_instance_from_instantiation_params(
        artifact,
        ContractInstantiationParams {
            constructor_name: effective_constructor_name.as_deref(),
            constructor_args: init_args.clone(),
            salt,
            public_keys: public_keys.clone(),
            deployer: AztecAddress::zero(),
        },
    )
    .expect("compute instance");
    // Publish instance on-chain
    let interaction = publish_instance(wallet, &instance).expect("publish_instance interaction");
    interaction
        .send(SendOptions {
            from,
            ..Default::default()
        })
        .await
        .expect("publish instance tx");
    wallet
        .wait_for_contract(instance.address)
        .await
        .expect("wait for published instance");

    // Register directly with the PXE contract store. The wallet's
    // register_contract may skip local registration when the node already
    // has the instance (the PXE get_contract_instance falls back to the
    // node, so the wallet thinks it's already registered locally).
    wallet
        .pxe()
        .register_contract(RegisterContractRequest {
            instance: instance.clone(),
            artifact: Some(artifact.clone()),
        })
        .await
        .expect("register contract locally");

    (instance, init_args)
}

/// Helper: create and publish instance via a TestContract's
/// `publish_contract_instance` method instead of from the wallet directly.
async fn create_and_publish_instance_via_contract(
    wallet: &TestWallet,
    stateful_artifact: &ContractArtifact,
    test_contract_artifact: &ContractArtifact,
    test_contract_address: AztecAddress,
    init_args: Vec<AbiValue>,
    constructor_name: Option<&str>,
    from: AztecAddress,
) -> (ContractInstanceWithAddress, Vec<AbiValue>) {
    let salt = Fr::from(next_unique_salt());
    let public_keys = PublicKeys {
        master_nullifier_public_key: grumpkin::scalar_mul(
            &aztec_rs::types::Fq::random(),
            &grumpkin::generator(),
        ),
        master_incoming_viewing_public_key: grumpkin::scalar_mul(
            &aztec_rs::types::Fq::random(),
            &grumpkin::generator(),
        ),
        master_outgoing_viewing_public_key: grumpkin::scalar_mul(
            &aztec_rs::types::Fq::random(),
            &grumpkin::generator(),
        ),
        master_tagging_public_key: grumpkin::scalar_mul(
            &aztec_rs::types::Fq::random(),
            &grumpkin::generator(),
        ),
    };
    let effective_constructor_name = constructor_name
        .map(str::to_owned)
        .or_else(|| default_initializer_name(stateful_artifact));

    let instance = get_contract_instance_from_instantiation_params(
        stateful_artifact,
        ContractInstantiationParams {
            constructor_name: effective_constructor_name.as_deref(),
            constructor_args: init_args.clone(),
            salt,
            public_keys: public_keys.clone(),
            deployer: AztecAddress::zero(),
        },
    )
    .expect("compute instance");
    wallet
        .pxe()
        .register_contract(RegisterContractRequest {
            instance: instance.clone(),
            artifact: Some(stateful_artifact.clone()),
        })
        .await
        .expect("register contract locally");

    // Publish via TestContract.publish_contract_instance(address)
    let call = make_call(
        test_contract_artifact,
        test_contract_address,
        "publish_contract_instance",
        vec![abi_address(instance.address)],
    );
    send_call(wallet, call, from).await;
    wallet
        .wait_for_contract(instance.address)
        .await
        .expect("wait for published instance");

    (instance, init_args)
}

// ---------------------------------------------------------------------------
// Shared test state
// ---------------------------------------------------------------------------

struct SharedState {
    wallet: TestWallet,
    default_account_address: AztecAddress,
    stateful_artifact: ContractArtifact,
    test_artifact: ContractArtifact,
    /// Address of a deployed TestContract (used for "deploy from contract" tests).
    test_contract_address: AztecAddress,
    /// The contract class ID of the published StatefulTestContract.
    stateful_class_id: Fr,
}

static SHARED_STATE: OnceCell<Option<SharedState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static SharedState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<SharedState> {
    let (wallet, default_account_address) = setup_wallet(TEST_ACCOUNT_0).await?;

    let stateful_artifact = load_stateful_test_artifact();
    let test_artifact = load_test_contract_artifact();
    let stateful_class_id =
        compute_contract_class_id_from_artifact(&stateful_artifact).expect("class id");

    wallet
        .pxe()
        .register_contract_class(&stateful_artifact)
        .await
        .expect("register StatefulTest class locally");

    // Always attempt publication — the class may exist from a previous run
    // with stale bytecode. If the nullifier already exists the node will
    // reject the duplicate, which we tolerate.
    let interaction = publish_contract_class(&wallet, &stateful_artifact)
        .await
        .expect("publish_contract_class interaction");
    if let Err(err) = interaction
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
    {
        let err_str = err.to_string().to_lowercase();
        assert!(
            err_str.contains("existing nullifier") || err_str.contains("dropped"),
            "publish class tx: {err}"
        );
    }

    // Deploy a TestContract instance (used for "deploy from contract" tests)
    let test_class_id =
        compute_contract_class_id_from_artifact(&test_artifact).expect("test class id");
    wallet
        .pxe()
        .register_contract_class(&test_artifact)
        .await
        .expect("register TestContract class locally");
    let skip_test_class_publication = wallet
        .pxe()
        .node()
        .get_contract_class(&test_class_id)
        .await
        .expect("get TestContract class before deploy")
        .is_some();
    let deploy =
        Contract::deploy(&wallet, test_artifact.clone(), vec![], None).expect("deploy builder");
    let deploy_opts = DeployOptions {
        contract_address_salt: Some(Fr::from(next_unique_salt())),
        skip_class_publication: skip_test_class_publication,
        ..Default::default()
    };
    let result = match deploy
        .send(
            &deploy_opts,
            SendOptions {
                from: default_account_address,
                ..Default::default()
            },
        )
        .await
    {
        Ok(result) => result,
        Err(err)
            if err
                .to_string()
                .to_lowercase()
                .contains("existing nullifier") =>
        {
            deploy
                .send(
                    &DeployOptions {
                        skip_class_publication: true,
                        ..deploy_opts
                    },
                    SendOptions {
                        from: default_account_address,
                        ..Default::default()
                    },
                )
                .await
                .expect("deploy TestContract without class publication")
        }
        Err(err) => panic!("deploy TestContract: {err}"),
    };
    let test_contract_address = result.instance.address;

    // Verify the class is registered
    let class_info = wallet
        .pxe()
        .node()
        .get_contract_class(&stateful_class_id)
        .await
        .expect("get_contract_class");
    assert!(
        class_info.is_some(),
        "StatefulTestContract class should be registered on the node"
    );

    Some(SharedState {
        wallet,
        default_account_address,
        stateful_artifact,
        test_artifact,
        test_contract_address,
        stateful_class_id,
    })
}

// ===========================================================================
// Tests: publishing a contract class
// ===========================================================================

/// TS: publishing a contract class > registers the contract class on the node
///
/// Verifies that after publishing the StatefulTestContract class, the node
/// returns the registered class with matching artifact hash and private
/// functions root.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn registers_contract_class_on_node() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let registered = s
        .wallet
        .pxe()
        .node()
        .get_contract_class(&s.stateful_class_id)
        .await
        .expect("get_contract_class");

    assert!(registered.is_some(), "class should be registered");
}

/// TS: publishing a contract class > emits public bytecode
///
/// Publishes the TestContract class (different from StatefulTest) and
/// verifies the publication succeeded.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn emits_public_bytecode() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let test_class_id =
        compute_contract_class_id_from_artifact(&s.test_artifact).expect("test class id");

    // The TestContract class was published as part of deploying the TestContract
    // in shared state. Verify it is registered.
    let registered = s
        .wallet
        .pxe()
        .node()
        .get_contract_class(&test_class_id)
        .await
        .expect("get_contract_class");

    assert!(
        registered.is_some(),
        "TestContract class should be registered after deploy"
    );
}

// ===========================================================================
// Tests: deploying a contract instance from a wallet — private constructor
// ===========================================================================

/// TS: deploying a contract instance from a wallet > using a private constructor >
///     stores contract instance in the aztec node
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn wallet_private_stores_instance_on_node() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let init_args = stateful_ctor_args(s.default_account_address, 42);
    let (instance, _) = create_and_publish_instance(
        &s.wallet,
        &s.stateful_artifact,
        init_args,
        None, // default private constructor
        s.default_account_address,
    )
    .await;

    let deployed = s
        .wallet
        .pxe()
        .node()
        .get_contract(&instance.address)
        .await
        .expect("get_contract");

    let deployed = deployed.expect("contract should be deployed");
    assert_eq!(deployed.address, instance.address);
    assert_eq!(
        deployed.inner.current_contract_class_id,
        instance.inner.current_contract_class_id
    );
    assert_eq!(
        deployed.inner.initialization_hash,
        instance.inner.initialization_hash
    );
    assert_eq!(deployed.inner.salt, instance.inner.salt);
    assert_eq!(deployed.inner.deployer, instance.inner.deployer);
}

/// TS: deploying a contract instance from a wallet > using a private constructor >
///     calls a public function with no init check on the deployed instance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn wallet_private_calls_public_no_init_check() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let init_args = stateful_ctor_args(s.default_account_address, 42);
    let (instance, _) = create_and_publish_instance(
        &s.wallet,
        &s.stateful_artifact,
        init_args,
        None,
        s.default_account_address,
    )
    .await;

    let whom = random_valid_address();
    let call = make_call(
        &s.stateful_artifact,
        instance.address,
        "increment_public_value_no_init_check",
        vec![abi_address(whom), abi_field(10)],
    );
    send_call(&s.wallet, call, s.default_account_address).await;

    let value = read_public_value(
        &s.wallet,
        &s.stateful_artifact,
        instance.address,
        whom,
        s.default_account_address,
    )
    .await;
    assert_eq!(value, 10, "public value should be 10");
}

/// TS: deploying a contract instance from a wallet > using a private constructor >
///     refuses to call a public function with init check if the instance is not initialized
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn wallet_private_refuses_public_with_init_check() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let init_args = stateful_ctor_args(s.default_account_address, 42);
    let (instance, _) = create_and_publish_instance(
        &s.wallet,
        &s.stateful_artifact,
        init_args,
        None,
        s.default_account_address,
    )
    .await;

    let whom = random_valid_address();
    let call = make_call(
        &s.stateful_artifact,
        instance.address,
        "increment_public_value",
        vec![abi_address(whom), abi_field(10)],
    );

    simulate_should_fail(
        &s.wallet,
        call,
        s.default_account_address,
        &["not initialized", "reverted", "Assertion failed"],
    )
    .await;
}

/// TS: deploying a contract instance from a wallet > using a private constructor >
///     refuses to initialize the instance with wrong args via a private function
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn wallet_private_refuses_wrong_args_init() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let init_args = stateful_ctor_args(s.default_account_address, 42);
    let (instance, _) = create_and_publish_instance(
        &s.wallet,
        &s.stateful_artifact,
        init_args,
        None,
        s.default_account_address,
    )
    .await;

    // Try to init with wrong second arg (43 instead of 42)
    let wrong_call = make_call(
        &s.stateful_artifact,
        instance.address,
        "constructor",
        stateful_ctor_args(random_valid_address(), 43),
    );

    simulate_should_fail(
        &s.wallet,
        wrong_call,
        s.default_account_address,
        &[
            "initialization hash does not match",
            "Initialization hash",
            "Cannot satisfy constraint",
        ],
    )
    .await;
}

/// TS: deploying a contract instance from a wallet > using a private constructor >
///     initializes the contract and calls a public function
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn wallet_private_initializes_and_calls_public() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let init_args = stateful_ctor_args(s.default_account_address, 42);
    let (instance, init_args) = create_and_publish_instance(
        &s.wallet,
        &s.stateful_artifact,
        init_args,
        None,
        s.default_account_address,
    )
    .await;

    // Initialize
    let ctor_call = make_call(
        &s.stateful_artifact,
        instance.address,
        "constructor",
        init_args,
    );
    send_call(&s.wallet, ctor_call, s.default_account_address).await;

    // Now call a public function that requires initialization
    let whom = random_valid_address();
    let call = make_call(
        &s.stateful_artifact,
        instance.address,
        "increment_public_value",
        vec![abi_address(whom), abi_field(10)],
    );
    send_call(&s.wallet, call, s.default_account_address).await;

    let value = read_public_value(
        &s.wallet,
        &s.stateful_artifact,
        instance.address,
        whom,
        s.default_account_address,
    )
    .await;
    assert_eq!(value, 10, "public value should be 10 after init");
}

/// TS: deploying a contract instance from a wallet > using a private constructor >
///     refuses to reinitialize the contract
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn wallet_private_refuses_reinit() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let init_args = stateful_ctor_args(s.default_account_address, 42);
    let (instance, init_args) = create_and_publish_instance(
        &s.wallet,
        &s.stateful_artifact,
        init_args,
        None,
        s.default_account_address,
    )
    .await;

    // Initialize (first time — should succeed)
    let ctor_call = make_call(
        &s.stateful_artifact,
        instance.address,
        "constructor",
        init_args.clone(),
    );
    send_call(&s.wallet, ctor_call, s.default_account_address).await;

    // Re-initialize (should fail with duplicate nullifier)
    let ctor_call2 = make_call(
        &s.stateful_artifact,
        instance.address,
        "constructor",
        init_args,
    );
    send_call_should_fail(
        &s.wallet,
        ctor_call2,
        s.default_account_address,
        DUPLICATE_NULLIFIER_ERROR,
    )
    .await;
}

// ===========================================================================
// Tests: deploying a contract instance from a wallet — public constructor
// ===========================================================================

/// TS: deploying a contract instance from a wallet > using a public constructor >
///     refuses to initialize the instance with wrong args via a public function
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn wallet_public_refuses_wrong_args_init() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let init_args = stateful_ctor_args(s.default_account_address, 42);
    let (instance, _) = create_and_publish_instance(
        &s.wallet,
        &s.stateful_artifact,
        init_args,
        Some("public_constructor"),
        s.default_account_address,
    )
    .await;

    let whom = random_valid_address();
    let wrong_call = make_call(
        &s.stateful_artifact,
        instance.address,
        "public_constructor",
        stateful_ctor_args(whom, 43),
    );

    simulate_should_fail(
        &s.wallet,
        wrong_call,
        s.default_account_address,
        &[
            "initialization hash does not match",
            "reverted",
            "Assertion failed",
        ],
    )
    .await;
}

/// TS: deploying a contract instance from a wallet > using a public constructor >
///     initializes the contract and calls a public function
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn wallet_public_initializes_and_calls_public() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let init_args = stateful_ctor_args(s.default_account_address, 42);
    let (instance, init_args) = create_and_publish_instance(
        &s.wallet,
        &s.stateful_artifact,
        init_args,
        Some("public_constructor"),
        s.default_account_address,
    )
    .await;

    // Initialize via public constructor
    let ctor_call = make_call(
        &s.stateful_artifact,
        instance.address,
        "public_constructor",
        init_args,
    );
    send_call(&s.wallet, ctor_call, s.default_account_address).await;

    // Call a public function requiring initialization
    let whom = random_valid_address();
    let call = make_call(
        &s.stateful_artifact,
        instance.address,
        "increment_public_value",
        vec![abi_address(whom), abi_field(10)],
    );
    send_call(&s.wallet, call, s.default_account_address).await;

    let value = read_public_value(
        &s.wallet,
        &s.stateful_artifact,
        instance.address,
        whom,
        s.default_account_address,
    )
    .await;
    assert_eq!(value, 10, "public value should be 10 after init");
}

/// TS: deploying a contract instance from a wallet > using a public constructor >
///     refuses to reinitialize the contract
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn wallet_public_refuses_reinit() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let init_args = stateful_ctor_args(s.default_account_address, 42);
    let (instance, init_args) = create_and_publish_instance(
        &s.wallet,
        &s.stateful_artifact,
        init_args,
        Some("public_constructor"),
        s.default_account_address,
    )
    .await;

    // Initialize (first time)
    let ctor_call = make_call(
        &s.stateful_artifact,
        instance.address,
        "public_constructor",
        init_args.clone(),
    );
    send_call(&s.wallet, ctor_call, s.default_account_address).await;

    // Re-initialize (should fail)
    let ctor_call2 = make_call(
        &s.stateful_artifact,
        instance.address,
        "public_constructor",
        init_args,
    );
    simulate_should_fail(
        &s.wallet,
        ctor_call2,
        s.default_account_address,
        DUPLICATE_NULLIFIER_ERROR,
    )
    .await;
}

// ===========================================================================
// Tests: deploying a contract instance from a contract — private constructor
// ===========================================================================

/// TS: deploying a contract instance from a contract > using a private constructor >
///     stores contract instance in the aztec node
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn contract_private_stores_instance_on_node() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let init_args = stateful_ctor_args(s.default_account_address, 42);
    let (instance, _) = create_and_publish_instance_via_contract(
        &s.wallet,
        &s.stateful_artifact,
        &s.test_artifact,
        s.test_contract_address,
        init_args,
        None,
        s.default_account_address,
    )
    .await;

    let deployed = s
        .wallet
        .pxe()
        .node()
        .get_contract(&instance.address)
        .await
        .expect("get_contract");

    let deployed = deployed.expect("contract should be deployed");
    assert_eq!(deployed.address, instance.address);
    assert_eq!(
        deployed.inner.current_contract_class_id,
        instance.inner.current_contract_class_id
    );
}

/// TS: deploying a contract instance from a contract > using a private constructor >
///     initializes the contract and calls a public function
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn contract_private_initializes_and_calls_public() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let init_args = stateful_ctor_args(s.default_account_address, 42);
    let (instance, init_args) = create_and_publish_instance_via_contract(
        &s.wallet,
        &s.stateful_artifact,
        &s.test_artifact,
        s.test_contract_address,
        init_args,
        None,
        s.default_account_address,
    )
    .await;

    // Initialize
    let ctor_call = make_call(
        &s.stateful_artifact,
        instance.address,
        "constructor",
        init_args,
    );
    send_call(&s.wallet, ctor_call, s.default_account_address).await;

    // Call public function requiring init
    let whom = random_valid_address();
    let call = make_call(
        &s.stateful_artifact,
        instance.address,
        "increment_public_value",
        vec![abi_address(whom), abi_field(10)],
    );
    send_call(&s.wallet, call, s.default_account_address).await;

    let value = read_public_value(
        &s.wallet,
        &s.stateful_artifact,
        instance.address,
        whom,
        s.default_account_address,
    )
    .await;
    assert_eq!(value, 10, "public value should be 10 after init");
}

// ===========================================================================
// Tests: deploying a contract instance from a contract — public constructor
// ===========================================================================

/// TS: deploying a contract instance from a contract > using a public constructor >
///     initializes the contract and calls a public function
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn contract_public_initializes_and_calls_public() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let init_args = stateful_ctor_args(s.default_account_address, 42);
    let (instance, init_args) = create_and_publish_instance_via_contract(
        &s.wallet,
        &s.stateful_artifact,
        &s.test_artifact,
        s.test_contract_address,
        init_args,
        Some("public_constructor"),
        s.default_account_address,
    )
    .await;

    // Initialize via public constructor
    let ctor_call = make_call(
        &s.stateful_artifact,
        instance.address,
        "public_constructor",
        init_args,
    );
    send_call(&s.wallet, ctor_call, s.default_account_address).await;

    // Call public function requiring init
    let whom = random_valid_address();
    let call = make_call(
        &s.stateful_artifact,
        instance.address,
        "increment_public_value",
        vec![abi_address(whom), abi_field(10)],
    );
    send_call(&s.wallet, call, s.default_account_address).await;

    let value = read_public_value(
        &s.wallet,
        &s.stateful_artifact,
        instance.address,
        whom,
        s.default_account_address,
    )
    .await;
    assert_eq!(value, 10, "public value should be 10 after init");
}

/// TS: deploying a contract instance from a contract > using a public constructor >
///     refuses to reinitialize the contract
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn contract_public_refuses_reinit() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let init_args = stateful_ctor_args(s.default_account_address, 42);
    let (instance, init_args) = create_and_publish_instance_via_contract(
        &s.wallet,
        &s.stateful_artifact,
        &s.test_artifact,
        s.test_contract_address,
        init_args,
        Some("public_constructor"),
        s.default_account_address,
    )
    .await;

    // Initialize (first time)
    let ctor_call = make_call(
        &s.stateful_artifact,
        instance.address,
        "public_constructor",
        init_args.clone(),
    );
    send_call(&s.wallet, ctor_call, s.default_account_address).await;

    // Re-initialize (should fail)
    let ctor_call2 = make_call(
        &s.stateful_artifact,
        instance.address,
        "public_constructor",
        init_args,
    );
    simulate_should_fail(
        &s.wallet,
        ctor_call2,
        s.default_account_address,
        DUPLICATE_NULLIFIER_ERROR,
    )
    .await;
}

// ===========================================================================
// Tests: error scenarios in deployment
// ===========================================================================

/// TS: error scenarios in deployment > app logic call to an undeployed contract
///     reverts, but can be included
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn app_logic_call_to_undeployed_contract_reverts() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let whom = s.default_account_address;
    let init_args = stateful_ctor_args(whom, 42);

    // Register the contract locally but do NOT publish it on-chain
    let salt = Fr::from(next_unique_salt());
    let instance = get_contract_instance_from_instantiation_params(
        &s.stateful_artifact,
        ContractInstantiationParams {
            constructor_name: None,
            constructor_args: init_args,
            salt,
            public_keys: PublicKeys {
                master_nullifier_public_key: grumpkin::scalar_mul(
                    &aztec_rs::types::Fq::random(),
                    &grumpkin::generator(),
                ),
                master_incoming_viewing_public_key: grumpkin::scalar_mul(
                    &aztec_rs::types::Fq::random(),
                    &grumpkin::generator(),
                ),
                master_outgoing_viewing_public_key: grumpkin::scalar_mul(
                    &aztec_rs::types::Fq::random(),
                    &grumpkin::generator(),
                ),
                master_tagging_public_key: grumpkin::scalar_mul(
                    &aztec_rs::types::Fq::random(),
                    &grumpkin::generator(),
                ),
            },
            deployer: AztecAddress::zero(),
        },
    )
    .expect("compute instance");

    s.wallet
        .pxe()
        .register_contract_class(&s.stateful_artifact)
        .await
        .expect("register class locally");
    s.wallet
        .pxe()
        .register_contract(RegisterContractRequest {
            instance: instance.clone(),
            artifact: Some(s.stateful_artifact.clone()),
        })
        .await
        .expect("register contract locally");

    // Try to call a function on the undeployed contract — should fail
    let call = make_call(
        &s.stateful_artifact,
        instance.address,
        "increment_public_value_no_init_check",
        vec![abi_address(whom), abi_field(10)],
    );

    simulate_should_fail(
        &s.wallet,
        call,
        s.default_account_address,
        &["not deployed", "reverted"],
    )
    .await;
}
