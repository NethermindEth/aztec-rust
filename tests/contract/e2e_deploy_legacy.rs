//! Legacy deploy tests -- 1:1 mirror of upstream
//! `end-to-end/src/e2e_deploy_contract/legacy.test.ts`.
//!
//! Exercises the legacy deployment codepath using the basic Test contract:
//! deploy once, deploy consecutively, deploy and interact, duplicate-salt
//! rejection, and concurrent good/bad deploy with public-part revert.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_deploy_legacy -- --ignored --nocapture
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

use crate::common::*;

use aztec_rs::contract::Contract;

// ---------------------------------------------------------------------------
// Shared wallet (single account, as in upstream `DeployTest.setup()`)
// ---------------------------------------------------------------------------

static WALLET: OnceCell<Option<(TestWallet, AztecAddress)>> = OnceCell::const_new();

async fn get_wallet() -> Option<&'static (TestWallet, AztecAddress)> {
    WALLET
        .get_or_init(|| async { setup_wallet(TEST_ACCOUNT_0).await })
        .await
        .as_ref()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Deploy the Test contract with a specific salt.  Returns the deployed
/// address and instance.
async fn deploy_test_with_salt(
    wallet: &TestWallet,
    from: AztecAddress,
    salt: Fr,
) -> Result<(AztecAddress, ContractInstanceWithAddress), String> {
    Contract::deploy(wallet, load_test_contract_artifact(), vec![], None)
        .map_err(|e| e.to_string())?
        .send(
            &DeployOptions {
                contract_address_salt: Some(salt),
                ..Default::default()
            },
            SendOptions {
                from,
                ..Default::default()
            },
        )
        .await
        .map(|r| (r.instance.address, r.instance))
        .map_err(|e| e.to_string())
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: should deploy a test contract
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn should_deploy_a_test_contract() {
    let _guard = serial_guard();
    let Some((wallet, account)) = get_wallet().await else {
        return;
    };

    let salt = Fr::from(next_unique_salt());
    let (address, instance) = deploy_test_with_salt(wallet, *account, salt)
        .await
        .expect("deploy should succeed");

    // The computed address from the instance must match the receipt address.
    let computed = aztec_rs::hash::compute_contract_address_from_instance(&instance.inner)
        .expect("compute instance address");
    assert_eq!(address, computed, "receipt address must match instance");

    // Registered and published on-chain.
    let metadata = wallet
        .get_contract_metadata(address)
        .await
        .expect("get_contract_metadata");
    assert!(
        metadata.instance.is_some(),
        "instance should be known to the PXE after deploy"
    );
    assert!(
        metadata.is_contract_published,
        "contract class should be published on-chain"
    );
}

/// TS: should deploy one contract after another in consecutive rollups
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn should_deploy_one_contract_after_another_in_consecutive_rollups() {
    let _guard = serial_guard();
    let Some((wallet, account)) = get_wallet().await else {
        return;
    };

    for i in 0..2 {
        let salt = Fr::from(next_unique_salt());
        deploy_test_with_salt(wallet, *account, salt)
            .await
            .unwrap_or_else(|e| panic!("deploy #{i}: {e}"));
    }
}

/// TS: should deploy multiple contracts and interact with them
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn should_deploy_multiple_contracts_and_interact_with_them() {
    let _guard = serial_guard();
    let Some((wallet, account)) = get_wallet().await else {
        return;
    };

    let test_artifact = load_test_contract_artifact();
    for i in 0..2 {
        let salt = Fr::from(next_unique_salt());
        let (address, _instance) = deploy_test_with_salt(wallet, *account, salt)
            .await
            .unwrap_or_else(|e| panic!("deploy #{i}: {e}"));

        // Interact: call get_master_incoming_viewing_public_key(account)
        let call = build_call(
            &test_artifact,
            address,
            "get_master_incoming_viewing_public_key",
            vec![abi_address(*account)],
        );
        send_call(wallet, call, *account).await;
    }
}

/// TS: should not deploy a contract with the same salt twice
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn should_not_deploy_a_contract_with_the_same_salt_twice() {
    let _guard = serial_guard();
    let Some((wallet, account)) = get_wallet().await else {
        return;
    };

    let salt = Fr::from(next_unique_salt());
    deploy_test_with_salt(wallet, *account, salt)
        .await
        .expect("first deploy should succeed");

    let err = deploy_test_with_salt(wallet, *account, salt)
        .await
        .expect_err("second deploy with same salt should fail");

    let lower = err.to_lowercase();
    assert!(
        lower.contains("nullifier") || lower.contains("existing"),
        "expected existing-nullifier error, got: {err}"
    );
}

/// TS: should not deploy a contract which failed the public part of the execution
///
/// Sends a "good" deploy (StatefulTest with skip_class/skip_instance
/// publication) and a "bad" deploy (Token with `AztecAddress::ZERO` as admin,
/// which reverts in the public constructor).  Both land in the chain — the
/// good one applies fully, the bad one is included but reverted in app-logic.
/// Specifically, the bad deploy must NOT register the contract class.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn should_not_deploy_a_contract_which_failed_public_part_of_execution() {
    let _guard = serial_guard();
    let Some((wallet, account)) = get_wallet().await else {
        return;
    };

    let stateful_artifact = load_stateful_test_artifact();
    let token_artifact = load_token_artifact();

    // Good deploy: StatefulTest(owner, value).
    //
    // The constructor's ABI declares `value` as `Field` (not an integer), so
    // we pass `AbiValue::Field(42)` rather than `AbiValue::Integer(42)` —
    // the Rust ABI encoder is strict about Field-typed params.  Upstream TS
    // coerces `number` into a Field via aztec.js's ABI codec, but there's no
    // analogous coercion here.  Mirrors upstream `StatefulTestContract.deploy(
    // wallet, defaultAccountAddress, 42)`.
    let good_result = Contract::deploy(
        wallet,
        stateful_artifact.clone(),
        vec![
            AbiValue::Field(Fr::from(*account)),
            AbiValue::Field(Fr::from(42u64)),
        ],
        None,
    )
    .expect("build good deploy")
    .send(
        &DeployOptions {
            contract_address_salt: Some(Fr::from(next_unique_salt())),
            skip_class_publication: true,
            skip_instance_publication: true,
            ..Default::default()
        },
        SendOptions {
            from: *account,
            ..Default::default()
        },
    )
    .await;
    assert!(
        good_result.is_ok(),
        "good deploy should succeed: {good_result:?}"
    );

    // Bad deploy: Token(ZERO, "TokenName", "TKN", 18) -- zero admin triggers
    // a public-constructor revert.
    let bad_deploy = Contract::deploy(
        wallet,
        token_artifact.clone(),
        vec![
            AbiValue::Field(Fr::zero()),
            AbiValue::String("TokenName".to_owned()),
            AbiValue::String("TKN".to_owned()),
            AbiValue::Integer(18),
        ],
        None,
    )
    .expect("build bad deploy");

    let bad_salt = Fr::from(next_unique_salt());
    let bad_class_id = aztec_rs::hash::compute_contract_class_id_from_artifact(&token_artifact)
        .expect("compute class id");

    // Snapshot class-registration state BEFORE the bad deploy.  Upstream TS
    // runs against a per-`describe` fresh snapshot so the class is
    // guaranteed to start unregistered — a direct `false` assertion after
    // the fact suffices there.  Our Rust e2e tests share a single long-lived
    // sandbox across files, so the Token class may already be published by
    // another test (e.g. e2e_escrow_contract).  We instead assert that the
    // bad deploy does not *change* the registration state, which captures
    // the same upstream intent ("the reverted bad tx did not publish the
    // class") under either condition.
    let class_registered_before = wallet
        .get_contract_class_metadata(bad_class_id)
        .await
        .expect("get_contract_class_metadata before")
        .is_contract_class_publicly_registered;

    // Upstream uses `{ wait: { dontThrowOnRevert: true, returnReceipt: true } }`
    // so the test can keep going even when the tx lands reverted.  The Rust
    // SDK returns `Err` on revert (no equivalent flag yet), so we swallow
    // the error and check the post-state directly.
    let bad_result = bad_deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(bad_salt),
                ..Default::default()
            },
            SendOptions {
                from: *account,
                ..Default::default()
            },
        )
        .await;
    assert!(
        bad_result.is_err(),
        "bad deploy with zero admin must revert or be rejected: {bad_result:?}"
    );

    let class_registered_after = wallet
        .get_contract_class_metadata(bad_class_id)
        .await
        .expect("get_contract_class_metadata after")
        .is_contract_class_publicly_registered;
    assert_eq!(
        class_registered_before, class_registered_after,
        "bad deploy must not change class-registration state (class_id={bad_class_id})"
    );
}
