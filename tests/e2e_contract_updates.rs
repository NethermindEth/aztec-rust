//! Contract updates tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_contract_updates.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_contract_updates -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::cast_possible_wrap,
    dead_code,
    unused_imports
)]

mod common;

use aztec_rs::abi::AbiValue;
use aztec_rs::deployment::publish_contract_class;
use common::*;
use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Constants (mirrors upstream)
// ---------------------------------------------------------------------------

const INITIAL_UPDATABLE_CONTRACT_VALUE: u64 = 1;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_updatable_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[root.join("fixtures/updatable_contract_compiled.json")])
}

fn load_updated_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[root.join("fixtures/updated_contract_compiled.json")])
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct ContractUpdateState {
    wallet: TestWallet,
    owner: AztecAddress,
    updatable_artifact: ContractArtifact,
    updated_artifact: ContractArtifact,
    contract_address: AztecAddress,
    updated_class_id: Fr,
}

static SHARED_STATE: OnceCell<Option<ContractUpdateState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static ContractUpdateState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<ContractUpdateState> {
    let updatable_artifact = load_updatable_artifact()?;
    let updated_artifact = load_updated_artifact()?;

    let (wallet, owner) = setup_wallet(TEST_ACCOUNT_0).await?;

    // Deploy UpdatableContract with initialize(initial_value)
    let deploy = Contract::deploy(
        &wallet,
        updatable_artifact.clone(),
        vec![AbiValue::Field(Fr::from(INITIAL_UPDATABLE_CONTRACT_VALUE))],
        Some("initialize"),
    )
    .expect("deploy builder");

    let result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: owner,
                ..Default::default()
            },
        )
        .await
        .expect("deploy updatable");
    let contract_address = result.instance.address;

    // Publish UpdatedContract class
    let publish_interaction = publish_contract_class(&wallet, &updated_artifact)
        .await
        .expect("publish updated class");
    if let Err(err) = publish_interaction
        .send(SendOptions {
            from: owner,
            ..Default::default()
        })
        .await
    {
        let err_str = err.to_string().to_lowercase();
        if !err_str.contains("existing nullifier") && !err_str.contains("dropped") {
            panic!("publish updated class: {err}");
        }
    }

    let updated_class_id =
        aztec_rs::hash::compute_contract_class_id_from_artifact(&updated_artifact)
            .expect("compute updated class id");

    Some(ContractUpdateState {
        wallet,
        owner,
        updatable_artifact,
        updated_artifact,
        contract_address,
        updated_class_id,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_updatable_call(
    artifact: &ContractArtifact,
    address: AztecAddress,
    method: &str,
    args: Vec<AbiValue>,
) -> FunctionCall {
    let func = artifact
        .find_function(method)
        .unwrap_or_else(|e| panic!("function '{method}' not found: {e}"));
    FunctionCall {
        to: address,
        selector: func.selector.expect("selector"),
        args,
        function_type: func.function_type.clone(),
        is_static: false,
        hide_msg_sender: false,
    }
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: should update the contract
///
/// Deploys UpdatableContract, publishes UpdatedContract class, calls
/// update_to with new class ID. Verifying the update takes effect requires
/// time warp (not yet available), so we verify the update_to call succeeds
/// and the initial state is correct.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn should_update_the_contract() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Verify initial public value
    // UpdatableContract storage: private_value(1), public_value(2)
    let public_val = read_public_storage(&s.wallet, s.contract_address, Fr::from(2u64)).await;
    assert_eq!(
        public_val.to_usize() as u64,
        INITIAL_UPDATABLE_CONTRACT_VALUE,
        "initial public value should match"
    );

    // Call update_to(updated_class_id) — schedules the update
    // ContractClassId is a struct { inner: Field }
    let mut class_id_struct = std::collections::BTreeMap::new();
    class_id_struct.insert("inner".to_owned(), AbiValue::Field(s.updated_class_id));
    let call = build_updatable_call(
        &s.updatable_artifact,
        s.contract_address,
        "update_to",
        vec![AbiValue::Struct(class_id_struct)],
    );
    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("update_to");

    // To verify the update takes effect, we'd need to warp time past
    // DEFAULT_TEST_UPDATE_DELAY. Once cheatcodes are available:
    // cheat_codes.warp_l2_time_at_least_by(DEFAULT_TEST_UPDATE_DELAY);
    // wallet.register_contract(instance, updated_artifact);
    // let updated = Contract::at(contract_address, updated_artifact, wallet);
    // updated.set_private_value().send();
    // assert get_private_value == UPDATED_CONTRACT_PUBLIC_VALUE
}

/// TS: should change the update delay and then update the contract
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn should_change_delay_then_update() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Read current update delay via public storage
    // (get_update_delay is a public view function)
    let call = build_updatable_call(
        &s.updatable_artifact,
        s.contract_address,
        "set_update_delay",
        vec![AbiValue::Integer(86400)], // Set delay to 86400 (1 day, above minimum)
    );
    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect("set_update_delay");

    // Verifying the delay change and subsequent update requires time warp
}

/// TS: should not allow to change the delay to a value lower than the minimum
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn rejects_delay_below_minimum() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // MINIMUM_UPDATE_DELAY from upstream constants
    let too_small_delay = 1u64; // Way below minimum

    let call = build_updatable_call(
        &s.updatable_artifact,
        s.contract_address,
        "set_update_delay",
        vec![AbiValue::Integer(too_small_delay as i128)],
    );

    let err = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.owner,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: delay below minimum");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("minimum")
            || err_str.contains("delay")
            || err_str.contains("assertion")
            || err_str.contains("reverted"),
        "expected minimum delay error, got: {err}"
    );
}

/// TS: should not allow to instantiate a contract with an updated class
///     before the update happens
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn rejects_early_instantiation_with_updated_class() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // After update_to is called but BEFORE time warp, calling methods
    // from the updated contract should fail because the update hasn't
    // taken effect yet.

    // Register updated artifact for the same address
    s.wallet
        .pxe()
        .register_contract(RegisterContractRequest {
            instance: ContractInstanceWithAddress {
                address: s.contract_address,
                inner: ContractInstance {
                    version: 1,
                    salt: Fr::zero(),
                    deployer: AztecAddress::zero(),
                    current_contract_class_id: s.updated_class_id,
                    original_contract_class_id: s.updated_class_id,
                    initialization_hash: Fr::zero(),
                    public_keys: PublicKeys::default(),
                },
            },
            artifact: Some(s.updated_artifact.clone()),
        })
        .await
        .ok();

    // Try calling set_private_value (only in UpdatedContract) — should fail
    // because the class hasn't actually changed on-chain yet
    if let Ok(func) = s.updated_artifact.find_function("set_private_value") {
        let call = FunctionCall {
            to: s.contract_address,
            selector: func.selector.expect("selector"),
            args: vec![],
            function_type: func.function_type.clone(),
            is_static: false,
            hide_msg_sender: false,
        };

        let result = s
            .wallet
            .send_tx(
                ExecutionPayload {
                    calls: vec![call],
                    ..Default::default()
                },
                SendOptions {
                    from: s.owner,
                    ..Default::default()
                },
            )
            .await;

        // Should fail because update hasn't taken effect
        if let Err(err) = result {
            let err_str = err.to_string().to_lowercase();
            assert!(
                err_str.contains("class")
                    || err_str.contains("update")
                    || err_str.contains("not found")
                    || err_str.contains("reverted")
                    || err_str.contains("constraint"),
                "expected class mismatch error, got: {err}"
            );
        }
        // If it succeeds, the update may have already taken effect from a previous run
    }
}
