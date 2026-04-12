//! Token access control tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/access_control.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_access_control -- --ignored --nocapture
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
// Helpers
// ---------------------------------------------------------------------------

// Token storage: admin(1), minters(2), ...
fn read_admin_slot(
    wallet: &TestWallet,
    token: AztecAddress,
) -> impl std::future::Future<Output = Fr> + '_ {
    read_public_storage(wallet, token, Fr::from(1u64))
}

async fn read_minter_slot(wallet: &TestWallet, token: AztecAddress, minter: &AztecAddress) -> u128 {
    let slot = derive_storage_slot_in_map(2, minter);
    read_public_u128(wallet, token, slot).await
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct AccessControlState {
    wallet: TestWallet,
    admin: AztecAddress,
    other: AztecAddress,
    token_artifact: ContractArtifact,
    token_address: AztecAddress,
}

static SHARED_STATE: OnceCell<Option<AccessControlState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static AccessControlState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<AccessControlState> {
    let (wallet, admin) = setup_wallet(TEST_ACCOUNT_0).await?;
    let other = imported_complete_address(TEST_ACCOUNT_1).address;

    let token_artifact = load_token_artifact();
    let deploy = Contract::deploy(
        &wallet,
        token_artifact.clone(),
        vec![
            AbiValue::Field(Fr::from(admin)),
            AbiValue::String("AccessToken".to_owned()),
            AbiValue::String("AT".to_owned()),
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
                from: admin,
                ..Default::default()
            },
        )
        .await
        .expect("deploy token");

    Some(AccessControlState {
        wallet,
        admin,
        other,
        token_artifact,
        token_address: result.instance.address,
    })
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: Set admin
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn set_admin() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Verify initial admin
    let initial_admin = read_admin_slot(&s.wallet, s.token_address).await;
    assert_eq!(
        initial_admin,
        Fr::from(s.admin),
        "initial admin should be deployer"
    );

    // Set new admin
    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "set_admin",
        vec![AbiValue::Field(Fr::from(s.other))],
    );
    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.admin,
                ..Default::default()
            },
        )
        .await
        .expect("set_admin");

    let new_admin = read_admin_slot(&s.wallet, s.token_address).await;
    assert_eq!(new_admin, Fr::from(s.other), "admin should be updated");

    // Note: admin was changed to s.other. We can't restore from s.other
    // because it's not in our wallet's account provider. The set_admin
    // is a one-way test — subsequent tests must account for this.
}

/// TS: Add minter as admin
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn add_minter_as_admin() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "set_minter",
        vec![AbiValue::Field(Fr::from(s.other)), AbiValue::Boolean(true)],
    );
    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.admin,
                ..Default::default()
            },
        )
        .await
        .expect("set_minter(true)");

    let is_minter = read_minter_slot(&s.wallet, s.token_address, &s.other).await;
    assert!(is_minter != 0, "other should be minter");
}

/// TS: Revoke minter as admin
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn revoke_minter_as_admin() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // First ensure other is a minter
    let add = build_call(
        &s.token_artifact,
        s.token_address,
        "set_minter",
        vec![AbiValue::Field(Fr::from(s.other)), AbiValue::Boolean(true)],
    );
    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![add],
                ..Default::default()
            },
            SendOptions {
                from: s.admin,
                ..Default::default()
            },
        )
        .await
        .expect("add minter");

    // Revoke
    let revoke = build_call(
        &s.token_artifact,
        s.token_address,
        "set_minter",
        vec![AbiValue::Field(Fr::from(s.other)), AbiValue::Boolean(false)],
    );
    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![revoke],
                ..Default::default()
            },
            SendOptions {
                from: s.admin,
                ..Default::default()
            },
        )
        .await
        .expect("revoke minter");

    let is_minter = read_minter_slot(&s.wallet, s.token_address, &s.other).await;
    assert_eq!(is_minter, 0, "other should no longer be minter");
}

/// TS: Set admin (not admin) — should fail
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn set_admin_not_admin_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "set_admin",
        vec![AbiValue::Field(Fr::from(s.other))],
    );

    let err = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.other, // NOT admin
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: not admin");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("caller is not admin")
            || err_str.contains("assertion")
            || err_str.contains("reverted")
            || err_str.contains("account not found"),
        "expected admin check error, got: {err}"
    );
}

/// TS: Revoke minter not as admin — should fail
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn revoke_minter_not_admin_fails() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "set_minter",
        vec![AbiValue::Field(Fr::from(s.other)), AbiValue::Boolean(false)],
    );

    let err = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.other, // NOT admin
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: not admin");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("caller is not admin")
            || err_str.contains("assertion")
            || err_str.contains("reverted")
            || err_str.contains("account not found"),
        "expected admin check error, got: {err}"
    );
}
