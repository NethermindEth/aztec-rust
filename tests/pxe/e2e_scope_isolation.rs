//! Scope isolation tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_scope_isolation.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_scope_isolation -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::map_unwrap_or,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::single_match_else,
    clippy::too_many_lines,
    dead_code,
    unused_imports
)]

use crate::common::*;
use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Constants (mirrors upstream)
// ---------------------------------------------------------------------------

const ALICE_NOTE_VALUE: u64 = 42;
const BOB_NOTE_VALUE: u64 = 100;

// ---------------------------------------------------------------------------
// Extraction helpers (unique to scope tests)
// ---------------------------------------------------------------------------

/// Try to extract a Field return value from a simulate result.
/// Returns `None` if the return values are empty (e.g. PCPI-encoded returns
/// that the PXE doesn't yet surface as ACIR return values).
fn try_extract_simulate_value(result: &aztec_rs::wallet::TxSimulationResult) -> Option<u64> {
    let rv = result
        .return_values
        .get("returnValues")
        .unwrap_or(&result.return_values);
    if let Some(arr) = rv.as_array() {
        if arr.is_empty() {
            return None;
        }
    }
    Some(extract_field_value(rv))
}

/// Extract a Field return value (as u64) from a utility execution result.
fn extract_utility_value(result: &aztec_rs::wallet::UtilityExecutionResult) -> u64 {
    extract_field_value(&result.result)
}

/// Parse a Field value from various JSON formats returned by PXE.
#[allow(clippy::cast_possible_truncation)]
fn extract_field_value(value: &serde_json::Value) -> u64 {
    if let Some(s) = value.as_str() {
        return Fr::from_hex(s)
            .map(|f| f.to_usize() as u64)
            .unwrap_or_else(|_| panic!("parse hex string: {s}"));
    }
    if let Some(arr) = value.as_array() {
        if let Some(first) = arr.first() {
            return extract_field_value(first);
        }
    }
    if let Some(n) = value.as_u64() {
        return n;
    }
    panic!("unexpected return value format: {value:?}");
}

// ---------------------------------------------------------------------------
// Shared test state (mirrors beforeAll)
// ---------------------------------------------------------------------------

struct TestState {
    alice_wallet: TestWallet,
    bob_wallet: TestWallet,
    alice: AztecAddress,
    bob: AztecAddress,
    charlie: AztecAddress,
    contract_address: AztecAddress,
    artifact: ContractArtifact,
}

static SHARED_STATE: OnceCell<Option<TestState>> = OnceCell::const_new();

/// Get or initialize the shared test state.
/// Returns `None` when the node is not reachable (test is skipped).
async fn get_shared_state() -> Option<&'static TestState> {
    let state = SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await;
    state.as_ref()
}

/// Mirrors upstream `beforeAll`: setup 3 accounts, deploy contract, create notes.
async fn init_shared_state() -> Option<TestState> {
    // setup(3) → alice, bob, charlie
    let (alice_wallet, alice) =
        setup_wallet_with_accounts(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1, TEST_ACCOUNT_2]).await?;
    let (bob_wallet, bob) =
        setup_wallet_with_accounts(TEST_ACCOUNT_1, &[TEST_ACCOUNT_0, TEST_ACCOUNT_2]).await?;
    let charlie =
        AztecAddress(Fr::from_hex(TEST_ACCOUNT_2.address).expect("valid charlie address"));

    let artifact = load_scope_test_artifact();

    // Deploy ScopeTestContract from alice
    let deploy =
        Contract::deploy(&alice_wallet, artifact.clone(), vec![], None).expect("deploy builder");
    let deploy_result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: alice,
                ..Default::default()
            },
        )
        .await
        .expect("deploy ScopeTestContract");
    let contract_address = deploy_result.instance.address;

    // Register contract on bob's PXE so he can interact with it
    register_contract_on_pxe(bob_wallet.pxe(), &artifact, &deploy_result.instance).await;

    // Alice creates a note for herself
    let create_alice = build_call(
        &artifact,
        contract_address,
        "create_note",
        vec![
            abi_address(alice),
            AbiValue::Field(Fr::from(ALICE_NOTE_VALUE)),
        ],
    );
    alice_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![create_alice],
                ..Default::default()
            },
            SendOptions {
                from: alice,
                ..Default::default()
            },
        )
        .await
        .expect("alice create_note");

    // Bob creates a note for himself
    let create_bob = build_call(
        &artifact,
        contract_address,
        "create_note",
        vec![abi_address(bob), AbiValue::Field(Fr::from(BOB_NOTE_VALUE))],
    );
    bob_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![create_bob],
                ..Default::default()
            },
            SendOptions {
                from: bob,
                ..Default::default()
            },
        )
        .await
        .expect("bob create_note");

    // Trigger PXE contract sync on both wallets so that notes created above
    // are discovered and available for subsequent simulate/utility calls.
    // Calling any utility function on the contract triggers ensure_contract_synced.
    let sync_call = build_call(
        &artifact,
        contract_address,
        "read_note_utility",
        vec![abi_address(alice)],
    );
    let _ = alice_wallet
        .execute_utility(
            sync_call.clone(),
            ExecuteUtilityOptions {
                scope: alice,
                ..Default::default()
            },
        )
        .await;
    let _ = bob_wallet
        .execute_utility(
            sync_call,
            ExecuteUtilityOptions {
                scope: bob,
                ..Default::default()
            },
        )
        .await;

    Some(TestState {
        alice_wallet,
        bob_wallet,
        alice,
        bob,
        charlie,
        contract_address,
        artifact,
    })
}

// ===========================================================================
// describe('external private')
// ===========================================================================

/// TS: it('owner can read own notes')
///
/// Simulates `contract.methods.read_note(alice)` from alice's scope.
/// Expects result == `ALICE_NOTE_VALUE`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_private_owner_can_read_own_notes() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Simulate the private call — verifies it succeeds without error
    let call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note",
        vec![abi_address(s.alice)],
    );
    let result = s
        .alice_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.alice,
                ..Default::default()
            },
        )
        .await
        .expect("simulate read_note(alice) from alice");

    // Verify the return value (extract from simulation or cross-check via utility)
    let value = match try_extract_simulate_value(&result) {
        Some(v) => v,
        None => {
            let ucall = build_call(
                &s.artifact,
                s.contract_address,
                "read_note_utility",
                vec![abi_address(s.alice)],
            );
            let uresult = s
                .alice_wallet
                .execute_utility(
                    ucall,
                    ExecuteUtilityOptions {
                        scope: s.alice,
                        ..Default::default()
                    },
                )
                .await
                .expect("execute read_note_utility(alice) scope alice");
            extract_utility_value(&uresult)
        }
    };
    assert_eq!(
        value, ALICE_NOTE_VALUE,
        "expected {ALICE_NOTE_VALUE}, got {value}"
    );
}

/// TS: it('cannot read notes belonging to a different account')
///
/// Simulates `contract.methods.read_note(alice)` from bob's scope.
/// Expects rejection: "Failed to get a note".
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_private_cannot_read_notes_belonging_to_a_different_account() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note",
        vec![abi_address(s.alice)],
    );

    let err = s
        .bob_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.bob,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: bob cannot read alice's notes");

    assert!(
        err.to_string().contains("Failed to get a note")
            || err.to_string().contains("Failed to solve brillig function"),
        "expected 'Failed to get a note' error, got: {err}"
    );
}

/// TS: it('cannot access nullifier hiding key of a different account')
///
/// Simulates `contract.methods.get_nhk(charlie)` from bob's scope.
/// Expects rejection: "Key validation request denied".
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_private_cannot_access_nullifier_hiding_key_of_a_different_account() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = build_call(
        &s.artifact,
        s.contract_address,
        "get_nhk",
        vec![abi_address(s.charlie)],
    );

    let err = s
        .bob_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.bob,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: bob cannot access charlie's NHK");

    assert!(
        err.to_string().contains("Key validation request denied"),
        "expected 'Key validation request denied' error, got: {err}"
    );
}

/// TS: it('each account can access their isolated state on a shared wallet')
///
/// alice reads her note → `ALICE_NOTE_VALUE`
/// bob reads his note → `BOB_NOTE_VALUE`
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_private_each_account_can_access_their_isolated_state_on_a_shared_wallet() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Alice reads her own note
    let alice_call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note",
        vec![abi_address(s.alice)],
    );
    let alice_result = s
        .alice_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![alice_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.alice,
                ..Default::default()
            },
        )
        .await
        .expect("simulate read_note(alice) from alice");

    // Bob reads his own note
    let bob_call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note",
        vec![abi_address(s.bob)],
    );
    let bob_result = s
        .bob_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![bob_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.bob,
                ..Default::default()
            },
        )
        .await
        .expect("simulate read_note(bob) from bob");

    // Verify return values (cross-check via utility if PCPI extraction not available)
    let alice_value = match try_extract_simulate_value(&alice_result) {
        Some(v) => v,
        None => {
            let ucall = build_call(
                &s.artifact,
                s.contract_address,
                "read_note_utility",
                vec![abi_address(s.alice)],
            );
            let ur = s
                .alice_wallet
                .execute_utility(
                    ucall,
                    ExecuteUtilityOptions {
                        scope: s.alice,
                        ..Default::default()
                    },
                )
                .await
                .expect("execute read_note_utility(alice)");
            extract_utility_value(&ur)
        }
    };
    let bob_value = match try_extract_simulate_value(&bob_result) {
        Some(v) => v,
        None => {
            let ucall = build_call(
                &s.artifact,
                s.contract_address,
                "read_note_utility",
                vec![abi_address(s.bob)],
            );
            let ur = s
                .bob_wallet
                .execute_utility(
                    ucall,
                    ExecuteUtilityOptions {
                        scope: s.bob,
                        ..Default::default()
                    },
                )
                .await
                .expect("execute read_note_utility(bob)");
            extract_utility_value(&ur)
        }
    };

    assert_eq!(
        alice_value, ALICE_NOTE_VALUE,
        "expected alice value {ALICE_NOTE_VALUE}, got {alice_value}"
    );
    assert_eq!(
        bob_value, BOB_NOTE_VALUE,
        "expected bob value {BOB_NOTE_VALUE}, got {bob_value}"
    );
}

// ===========================================================================
// describe('external utility')
// ===========================================================================

/// TS: it('owner can read own notes')
///
/// Executes `contract.methods.read_note_utility(alice)` from alice's scope.
/// Expects result == `ALICE_NOTE_VALUE`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_utility_owner_can_read_own_notes() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note_utility",
        vec![abi_address(s.alice)],
    );

    let result = s
        .alice_wallet
        .execute_utility(
            call,
            ExecuteUtilityOptions {
                scope: s.alice,
                ..Default::default()
            },
        )
        .await
        .expect("execute read_note_utility(alice) scope alice");

    let value = extract_utility_value(&result);
    assert_eq!(
        value, ALICE_NOTE_VALUE,
        "expected {ALICE_NOTE_VALUE}, got {value}"
    );
}

/// TS: it('cannot read notes belonging to a different account')
///
/// Executes `contract.methods.read_note_utility(alice)` from bob's scope.
/// Expects rejection: "Failed to get a note".
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_utility_cannot_read_notes_belonging_to_a_different_account() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note_utility",
        vec![abi_address(s.alice)],
    );

    // Use alice's wallet (which has alice's notes) but with bob's scope
    let err = s
        .alice_wallet
        .execute_utility(
            call,
            ExecuteUtilityOptions {
                scope: s.bob,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: bob scope cannot read alice's notes");

    assert!(
        err.to_string().contains("Failed to get a note")
            || err.to_string().contains("Failed to solve brillig function"),
        "expected 'Failed to get a note' error, got: {err}"
    );
}

/// TS: it('cannot access nullifier hiding key of a different account')
///
/// Executes `contract.methods.get_nhk_utility(charlie)` from bob's scope.
/// Expects rejection: "Key validation request denied".
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_utility_cannot_access_nullifier_hiding_key_of_a_different_account() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = build_call(
        &s.artifact,
        s.contract_address,
        "get_nhk_utility",
        vec![abi_address(s.charlie)],
    );

    let err = s
        .alice_wallet
        .execute_utility(
            call,
            ExecuteUtilityOptions {
                scope: s.bob,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: bob scope cannot access charlie's NHK");

    assert!(
        err.to_string().contains("Key validation request denied"),
        "expected 'Key validation request denied' error, got: {err}"
    );
}

/// TS: it('each account can access their isolated state on a shared wallet')
///
/// alice reads via utility → `ALICE_NOTE_VALUE`
/// bob reads via utility → `BOB_NOTE_VALUE`
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn external_utility_each_account_can_access_their_isolated_state_on_a_shared_wallet() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Alice reads her own note via utility
    let alice_call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note_utility",
        vec![abi_address(s.alice)],
    );
    let alice_result = s
        .alice_wallet
        .execute_utility(
            alice_call,
            ExecuteUtilityOptions {
                scope: s.alice,
                ..Default::default()
            },
        )
        .await
        .expect("execute read_note_utility(alice) scope alice");

    // Bob reads his own note via utility
    let bob_call = build_call(
        &s.artifact,
        s.contract_address,
        "read_note_utility",
        vec![abi_address(s.bob)],
    );
    let bob_result = s
        .bob_wallet
        .execute_utility(
            bob_call,
            ExecuteUtilityOptions {
                scope: s.bob,
                ..Default::default()
            },
        )
        .await
        .expect("execute read_note_utility(bob) scope bob");

    let alice_value = extract_utility_value(&alice_result);
    let bob_value = extract_utility_value(&bob_result);

    assert_eq!(
        alice_value, ALICE_NOTE_VALUE,
        "expected alice value {ALICE_NOTE_VALUE}, got {alice_value}"
    );
    assert_eq!(
        bob_value, BOB_NOTE_VALUE,
        "expected bob value {BOB_NOTE_VALUE}, got {bob_value}"
    );
}
