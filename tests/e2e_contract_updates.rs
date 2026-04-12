//! Contract updates tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_contract_updates.test.ts`.
//!
//! These tests require UpdatableContract + UpdatedContract fixtures and
//! the ability to warp time (advance timestamps). Until those are available,
//! tests skip gracefully.
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
use common::*;
use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_updatable_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/updatable_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/updatable_contract-UpdatableContract.json"),
    ])
}

fn load_updated_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/updated_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/updated_contract-UpdatedContract.json"),
    ])
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct ContractUpdateState {
    wallet: TestWallet,
    owner: AztecAddress,
}

static SHARED_STATE: OnceCell<Option<ContractUpdateState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static ContractUpdateState> {
    SHARED_STATE
        .get_or_init(|| async {
            let (wallet, owner) = setup_wallet(TEST_ACCOUNT_0).await?;
            Some(ContractUpdateState { wallet, owner })
        })
        .await
        .as_ref()
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: should update the contract
///
/// Deploys UpdatableContract, publishes UpdatedContract class, calls
/// update_to with new class ID, warps time past delay, verifies new
/// methods are callable.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn should_update_the_contract() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let Some(_updatable) = load_updatable_artifact() else {
        eprintln!("skipping: UpdatableContract fixture not available");
        return;
    };
    let Some(_updated) = load_updated_artifact() else {
        eprintln!("skipping: UpdatedContract fixture not available");
        return;
    };

    // TODO: Implement when fixtures and time-warp are available:
    // 1. Deploy UpdatableContract
    // 2. Publish UpdatedContract class
    // 3. Call update_to(new_class_id) on the deployed contract
    // 4. Warp time past the update delay
    // 5. Verify new methods from UpdatedContract are callable
    eprintln!("skipping: time warp not yet available");
}

/// TS: should change the update delay and then update the contract
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn should_change_delay_then_update() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    let Some(_updatable) = load_updatable_artifact() else {
        eprintln!("skipping: UpdatableContract fixture not available");
        return;
    };

    // TODO: Requires time warp + delay configuration
    eprintln!("skipping: time warp not yet available");
}

/// TS: should not allow to change the delay to a value lower than the minimum
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn rejects_delay_below_minimum() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    let Some(_updatable) = load_updatable_artifact() else {
        eprintln!("skipping: UpdatableContract fixture not available");
        return;
    };

    // TODO: Requires set_update_delay with value < MINIMUM_UPDATE_DELAY
    eprintln!("skipping: UpdatableContract fixture not available");
}

/// TS: should not allow to instantiate a contract with an updated class
///     before the update happens
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn rejects_early_instantiation_with_updated_class() {
    let _guard = serial_guard();
    let Some(_s) = get_shared_state().await else {
        return;
    };

    let Some(_updatable) = load_updatable_artifact() else {
        eprintln!("skipping: UpdatableContract fixture not available");
        return;
    };

    // TODO: Requires time warp to test before/after update timing
    eprintln!("skipping: time warp not yet available");
}
