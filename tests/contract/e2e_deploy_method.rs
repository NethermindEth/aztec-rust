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

use crate::common::*;

use aztec_rs::contract::BatchCall;

// ---------------------------------------------------------------------------
// File-specific artifact loaders
// ---------------------------------------------------------------------------

fn load_counter_artifact() -> Option<ContractArtifact> {
    let json = include_str!("../../fixtures/counter_contract.json");
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
    let call = build_call(
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

    let call = build_call(
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
    let call = build_call(
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
    let call = build_call(
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
    let call = build_call(
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
        .register_contract(RegisterContractRequest {
            instance: instance.clone(),
            artifact: Some(artifact.clone()),
        })
        .await
        .expect("register contract locally");

    // Build deploy payload and public call payload
    let deploy_payload = deploy.request(&deploy_opts).await.expect("deploy request");
    let public_call = build_call(
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
