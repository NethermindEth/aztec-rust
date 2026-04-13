//! Show how PXE scopes affect note visibility.

#![allow(
    clippy::print_stdout,
    clippy::wildcard_imports,
    // `main` is long because the scope demo has many sequential steps.
    clippy::too_many_lines,
    // An expect_err inside main asserts that the blocked-read branch really errors.
    clippy::expect_used,
)]

mod common;

use common::*;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let Some((alice_wallet, alice)) =
        setup_wallet_with_accounts(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1, TEST_ACCOUNT_2]).await
    else {
        return Err(aztec_rs::Error::InvalidData(format!(
            "node not reachable at {}",
            node_url()
        )));
    };
    let Some((bob_wallet, bob)) =
        setup_wallet_with_accounts(TEST_ACCOUNT_1, &[TEST_ACCOUNT_0, TEST_ACCOUNT_2]).await
    else {
        return Err(aztec_rs::Error::InvalidData(format!(
            "node not reachable at {}",
            node_url()
        )));
    };
    let (contract_address, artifact, instance) =
        deploy_contract(&alice_wallet, load_scope_test_artifact(), vec![], alice).await?;
    register_contract_on_pxe(bob_wallet.pxe(), &artifact, &instance).await?;

    send_call(
        &alice_wallet,
        build_call(
            &artifact,
            contract_address,
            "create_note",
            vec![abi_address(alice), AbiValue::Field(Fr::from(42u64))],
        ),
        alice,
    )
    .await?;
    send_call(
        &bob_wallet,
        build_call(
            &artifact,
            contract_address,
            "create_note",
            vec![abi_address(bob), AbiValue::Field(Fr::from(100u64))],
        ),
        bob,
    )
    .await?;

    // Trigger PXE sync so the newly created notes are discoverable to utility calls.
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

    let alice_reads_alice = call_utility_u64(
        &alice_wallet,
        &artifact,
        contract_address,
        "read_note_utility",
        vec![abi_address(alice)],
        alice,
    )
    .await?;
    let bob_reads_bob = call_utility_u64(
        &bob_wallet,
        &artifact,
        contract_address,
        "read_note_utility",
        vec![abi_address(bob)],
        bob,
    )
    .await?;
    let blocked_read = alice_wallet
        .execute_utility(
            build_call(
                &artifact,
                contract_address,
                "read_note_utility",
                vec![abi_address(alice)],
            ),
            ExecuteUtilityOptions {
                scope: bob,
                ..Default::default()
            },
        )
        .await
        .expect_err("bob scope should not be able to read alice's note");
    let blocked_summary = blocked_read
        .to_string()
        .lines()
        .next()
        .unwrap_or("blocked")
        .to_owned();

    println!("Contract address:   {contract_address}");
    println!("Alice sees Alice:   {alice_reads_alice}");
    println!("Bob sees Bob:       {bob_reads_bob}");
    println!("Bob sees Alice:     blocked as expected (cross-scope note access denied)");
    println!("Isolation error:    {blocked_summary}");

    Ok(())
}
