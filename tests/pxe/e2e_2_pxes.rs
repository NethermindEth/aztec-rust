//! End-to-end tests with two PXE instances — 1:1 mirror of upstream
//! `end-to-end/src/e2e_2_pxes.test.ts`.
//!
//! All tests in this file require both a live Aztec node AND ACVM integration
//! (Phase 1) because they deploy contracts and execute transactions.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_2_pxes -- --ignored
//! ```

#![allow(
    clippy::expect_used,
    clippy::print_stderr,
    clippy::similar_names,
    dead_code,
    unused_variables
)]

use crate::common::*;

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

async fn setup_two_wallets() -> Option<((TestWallet, AztecAddress), (TestWallet, AztecAddress))> {
    let (wallet_a, account_a_address) = setup_wallet(TEST_ACCOUNT_0).await?;
    let (wallet_b, account_b_address) = setup_wallet(TEST_ACCOUNT_1).await?;

    wallet_a
        .pxe()
        .register_sender(&account_b_address)
        .await
        .expect("A registers B");
    wallet_b
        .pxe()
        .register_sender(&account_a_address)
        .await
        .expect("B registers A");

    Some(((wallet_a, account_a_address), (wallet_b, account_b_address)))
}

async fn deploy_child(
    wallet: &TestWallet,
    from: AztecAddress,
) -> (AztecAddress, ContractArtifact, ContractInstanceWithAddress) {
    let artifact = load_child_contract_artifact();
    deploy_contract(wallet, artifact, vec![], from).await
}

// ===========================================================================
// describe('e2e_2_pxes')
//
// All 5 tests below are 1:1 mirrors of the upstream TS e2e_2_pxes.test.ts.
// They all require ACVM integration (Phase 1) to actually run.
// ===========================================================================

/// TS: it('transfers funds from user A to B via PXE A followed by transfer
///        from B to A via PXE B')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfers_funds_from_user_a_to_b_via_pxe_a_followed_by_transfer_from_b_to_a_via_pxe_b() {
    let Some(((wallet_a, account_a_address), (wallet_b, account_b_address))) =
        setup_two_wallets().await
    else {
        return;
    };

    let initial_balance = 987u64;
    let transfer_amount1 = 654u64;
    let transfer_amount2 = 323u64;

    let (token_address, token_artifact, token_instance) =
        deploy_token(&wallet_a, account_a_address, initial_balance).await;

    // Add token to PXE B (PXE A already has it because it was deployed through it)
    register_contract_on_pxe(wallet_b.pxe(), &token_artifact, &token_instance).await;

    // Check initial balances are as expected
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        initial_balance,
    )
    .await;
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        account_b_address,
        0,
    )
    .await;

    // Transfer funds from A to B via PXE A
    send_token_method(
        &wallet_a,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(account_b_address)),
            AbiValue::Integer(i128::from(transfer_amount1)),
        ],
        account_a_address,
    )
    .await;

    // Check balances are as expected
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        initial_balance - transfer_amount1,
    )
    .await;
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        account_b_address,
        transfer_amount1,
    )
    .await;

    // Transfer funds from B to A via PXE B
    send_token_method(
        &wallet_b,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(account_a_address)),
            AbiValue::Integer(i128::from(transfer_amount2)),
        ],
        account_b_address,
    )
    .await;

    // Check balances are as expected
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        initial_balance - transfer_amount1 + transfer_amount2,
    )
    .await;
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        account_b_address,
        transfer_amount1 - transfer_amount2,
    )
    .await;
}

/// TS: it('user calls a public function on a contract deployed by a different
///        user using a different PXE')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn user_calls_a_public_function_on_a_contract_deployed_by_a_different_user_using_a_different_pxe(
) {
    let Some(((wallet_a, account_a_address), (wallet_b, account_b_address))) =
        setup_two_wallets().await
    else {
        return;
    };

    // Deploy Child contract via PXE A
    let (child_address, child_artifact, child_instance) =
        deploy_child(&wallet_a, account_a_address).await;

    // Add Child to PXE B
    register_contract_on_pxe(wallet_b.pxe(), &child_artifact, &child_instance).await;

    let new_value_to_set = Fr::from(256u64);

    // Call pub_inc_value via PXE B
    send_token_method(
        &wallet_b,
        &child_artifact,
        child_address,
        "pub_inc_value",
        vec![AbiValue::Field(new_value_to_set)],
        account_b_address,
    )
    .await;

    // Verify public storage via node
    let stored_value_on_b = read_public_storage(&wallet_b, child_address, Fr::from(1u64)).await;
    assert_eq!(stored_value_on_b, new_value_to_set);

    let stored_value_on_a = read_public_storage(&wallet_a, child_address, Fr::from(1u64)).await;
    assert_eq!(stored_value_on_a, new_value_to_set);
}

/// TS: it('private state is "zero" when PXE does not have the account secret key')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_state_is_zero_when_pxe_does_not_have_the_account_secret_key() {
    let Some(((wallet_a, account_a_address), (wallet_b, account_b_address))) =
        setup_two_wallets().await
    else {
        return;
    };

    let user_a_balance = 100u64;
    let user_b_balance = 150u64;

    let (token_address, token_artifact, token_instance) =
        deploy_token(&wallet_a, account_a_address, user_a_balance).await;

    // Add token to PXE B
    register_contract_on_pxe(wallet_b.pxe(), &token_artifact, &token_instance).await;

    // Mint tokens to user B
    mint_tokens_to_private(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        account_b_address,
        user_b_balance,
    )
    .await;

    // Check that user A balance is 100 on server A
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        user_a_balance,
    )
    .await;
    // Check that user B balance is 150 on server B
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        account_b_address,
        user_b_balance,
    )
    .await;

    // CHECK THAT PRIVATE BALANCES ARE 0 WHEN ACCOUNT'S SECRET KEYS ARE NOT REGISTERED
    // Check that user A balance is 0 on server B
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        account_a_address,
        0,
    )
    .await;
    // Check that user B balance is 0 on server A
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_b_address,
        0,
    )
    .await;
}

/// TS: it('permits sending funds to a user before they have registered the contract')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn permits_sending_funds_to_a_user_before_they_have_registered_the_contract() {
    let Some(((wallet_a, account_a_address), (wallet_b, account_b_address))) =
        setup_two_wallets().await
    else {
        return;
    };

    let initial_balance = 987u64;
    let transfer_amount1 = 654u64;

    let (token_address, token_artifact, token_instance) =
        deploy_token(&wallet_a, account_a_address, initial_balance).await;

    // Check initial balances are as expected
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        initial_balance,
    )
    .await;
    // don't check userB yet

    // Transfer funds from A to B via PXE A
    send_token_method(
        &wallet_a,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(account_b_address)),
            AbiValue::Integer(i128::from(transfer_amount1)),
        ],
        account_a_address,
    )
    .await;

    // now add the contract and check balances
    register_contract_on_pxe(wallet_b.pxe(), &token_artifact, &token_instance).await;
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        initial_balance - transfer_amount1,
    )
    .await;
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        account_b_address,
        transfer_amount1,
    )
    .await;
}

/// TS: it('permits sending funds to a user, and spending them, before they
///        have registered the contract')
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
#[allow(clippy::too_many_lines)]
async fn permits_sending_funds_to_a_user_and_spending_them_before_they_have_registered_the_contract(
) {
    let Some(((wallet_a, account_a_address), (wallet_b, account_b_address))) =
        setup_two_wallets().await
    else {
        return;
    };

    let initial_balance = 987u64;
    let transfer_amount1 = 654u64;
    let transfer_amount2 = 323u64;

    // setup an account that is shared across PXEs — create a dedicated wallet
    let Some((wallet_shared, shared_account_address)) = setup_wallet(TEST_ACCOUNT_2).await else {
        return;
    };
    let shared_secret =
        Fr::from_hex(TEST_ACCOUNT_2.secret_key).expect("valid test account secret key");
    let shared_account = imported_complete_address(TEST_ACCOUNT_2);

    // Register the shared account keys on wallet A and B so they can discover notes
    wallet_a
        .pxe()
        .key_store()
        .add_account(&shared_secret)
        .await
        .expect("seed shared key on A");
    wallet_a
        .pxe()
        .address_store()
        .add(&shared_account)
        .await
        .expect("seed shared address on A");
    wallet_b
        .pxe()
        .key_store()
        .add_account(&shared_secret)
        .await
        .expect("seed shared key on B");
    wallet_b
        .pxe()
        .address_store()
        .add(&shared_account)
        .await
        .expect("seed shared address on B");
    // Register senders across all wallets for tag discovery
    wallet_shared
        .pxe()
        .register_sender(&account_a_address)
        .await
        .expect("shared registers A");
    wallet_shared
        .pxe()
        .register_sender(&account_b_address)
        .await
        .expect("shared registers B");
    // wallet_b needs the shared account as a sender to discover transfers from shared→b
    wallet_b
        .pxe()
        .register_sender(&shared_account_address)
        .await
        .expect("B registers shared");
    // wallet_a also needs shared as sender
    wallet_a
        .pxe()
        .register_sender(&shared_account_address)
        .await
        .expect("A registers shared");

    // deploy the contract on PXE A
    let (token_address, token_artifact, token_instance) =
        deploy_token(&wallet_a, account_a_address, initial_balance).await;

    // Transfer funds from A to Shared Wallet via PXE A
    send_token_method(
        &wallet_a,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(shared_account_address)),
            AbiValue::Integer(i128::from(transfer_amount1)),
        ],
        account_a_address,
    )
    .await;

    // Register the token on the shared wallet so it can execute transfer
    register_contract_on_pxe(wallet_shared.pxe(), &token_artifact, &token_instance).await;

    // Now send funds from Shared Wallet to B via the shared wallet
    send_token_method(
        &wallet_shared,
        &token_artifact,
        token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(account_b_address)),
            AbiValue::Integer(i128::from(transfer_amount2)),
        ],
        shared_account_address,
    )
    .await;

    // check balances from PXE-A's perspective
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        account_a_address,
        initial_balance - transfer_amount1,
    )
    .await;
    expect_token_balance(
        &wallet_a,
        token_address,
        &token_artifact,
        shared_account_address,
        transfer_amount1 - transfer_amount2,
    )
    .await;

    // now add the contract and check balances from PXE-B's perspective.
    // The process should be:
    // PXE-B had previously deferred the notes from A -> Shared, and Shared -> B
    // PXE-B adds the contract
    // PXE-B reprocesses the deferred notes, and sees the nullifier for A -> Shared
    register_contract_on_pxe(wallet_b.pxe(), &token_artifact, &token_instance).await;
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        account_b_address,
        transfer_amount2,
    )
    .await;
    expect_token_balance(
        &wallet_b,
        token_address,
        &token_artifact,
        shared_account_address,
        transfer_amount1 - transfer_amount2,
    )
    .await;
}
