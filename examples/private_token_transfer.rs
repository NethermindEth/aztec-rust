//! Private token transfer with one PXE and two registered accounts.

#![allow(clippy::print_stdout)]

mod common;

use common::*;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let Some((wallet, alice)) = setup_wallet_with_accounts(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1]).await
    else {
        return Err(aztec_rs::Error::InvalidData(format!(
            "node not reachable at {}",
            node_url()
        )));
    };
    let bob = imported_complete_address(TEST_ACCOUNT_1).address;

    wallet.pxe().register_sender(&bob).await?;

    let (token_address, token_artifact, _) = deploy_token(&wallet, alice, 100).await?;
    send_token_method(
        &wallet,
        &token_artifact,
        token_address,
        "transfer",
        vec![AbiValue::Field(Fr::from(bob)), AbiValue::Integer(25)],
        alice,
    )
    .await?;

    let alice_balance =
        private_token_balance(&wallet, &token_artifact, token_address, alice).await?;
    let bob_balance = private_token_balance(&wallet, &token_artifact, token_address, bob).await?;

    println!("Token address:      {token_address}");
    println!("Alice:              {alice}");
    println!("Bob:                {bob}");
    println!("Alice balance:      {alice_balance}");
    println!("Bob balance:        {bob_balance}");

    Ok(())
}
