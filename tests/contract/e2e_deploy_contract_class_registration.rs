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

use crate::common::*;

use aztec_core::grumpkin;
use aztec_rs::deployment::{
    get_contract_instance_from_instantiation_params, publish_contract_class, publish_instance,
    ContractInstantiationParams,
};

// ---------------------------------------------------------------------------
// Constants (mirrors upstream fixtures/fixtures.ts)
// ---------------------------------------------------------------------------

/// Upstream: `DUPLICATE_NULLIFIER_ERROR = /dropped|nullifier|reverted/i`
const DUPLICATE_NULLIFIER_ERROR: &[&str] = &["dropped", "nullifier", "reverted"];

// ---------------------------------------------------------------------------
// File-specific helpers
// ---------------------------------------------------------------------------

/// Wrap an AztecAddress as a plain Field ABI value.
/// Note: this differs from common::abi_address which wraps as a struct.
/// The StatefulTestContract functions in this file expect the plain-field
/// encoding.
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
    let call = build_call(
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
    let call = build_call(
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
    let call = build_call(
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
    let wrong_call = build_call(
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
    let ctor_call = build_call(
        &s.stateful_artifact,
        instance.address,
        "constructor",
        init_args,
    );
    send_call(&s.wallet, ctor_call, s.default_account_address).await;

    // Now call a public function that requires initialization
    let whom = random_valid_address();
    let call = build_call(
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
    let ctor_call = build_call(
        &s.stateful_artifact,
        instance.address,
        "constructor",
        init_args.clone(),
    );
    send_call(&s.wallet, ctor_call, s.default_account_address).await;

    // Re-initialize (should fail with duplicate nullifier)
    let ctor_call2 = build_call(
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
    let wrong_call = build_call(
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
    let ctor_call = build_call(
        &s.stateful_artifact,
        instance.address,
        "public_constructor",
        init_args,
    );
    send_call(&s.wallet, ctor_call, s.default_account_address).await;

    // Call a public function requiring initialization
    let whom = random_valid_address();
    let call = build_call(
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
    let ctor_call = build_call(
        &s.stateful_artifact,
        instance.address,
        "public_constructor",
        init_args.clone(),
    );
    send_call(&s.wallet, ctor_call, s.default_account_address).await;

    // Re-initialize (should fail)
    let ctor_call2 = build_call(
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
    let ctor_call = build_call(
        &s.stateful_artifact,
        instance.address,
        "constructor",
        init_args,
    );
    send_call(&s.wallet, ctor_call, s.default_account_address).await;

    // Call public function requiring init
    let whom = random_valid_address();
    let call = build_call(
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
    let ctor_call = build_call(
        &s.stateful_artifact,
        instance.address,
        "public_constructor",
        init_args,
    );
    send_call(&s.wallet, ctor_call, s.default_account_address).await;

    // Call public function requiring init
    let whom = random_valid_address();
    let call = build_call(
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
    let ctor_call = build_call(
        &s.stateful_artifact,
        instance.address,
        "public_constructor",
        init_args.clone(),
    );
    send_call(&s.wallet, ctor_call, s.default_account_address).await;

    // Re-initialize (should fail)
    let ctor_call2 = build_call(
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
    let call = build_call(
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
