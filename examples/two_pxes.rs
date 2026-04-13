//! Transfer private state across two embedded PXEs.

#![allow(clippy::print_stdout)]

mod common;

use common::*;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let Some((wallet_a, account_a)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return Err(aztec_rs::Error::InvalidData(format!(
            "node not reachable at {}",
            node_url()
        )));
    };
    let Some((wallet_b, account_b)) = setup_wallet(TEST_ACCOUNT_1).await else {
        return Err(aztec_rs::Error::InvalidData(format!(
            "node not reachable at {}",
            node_url()
        )));
    };

    wallet_a.pxe().register_sender(&account_b).await?;
    wallet_b.pxe().register_sender(&account_a).await?;

    let (token_address, token_artifact, token_instance) =
        deploy_token(&wallet_a, account_a, 100).await?;
    register_contract_on_pxe(wallet_b.pxe(), &token_artifact, &token_instance).await?;

    send_token_method(
        &wallet_a,
        &token_artifact,
        token_address,
        "transfer",
        vec![AbiValue::Field(Fr::from(account_b)), AbiValue::Integer(30)],
        account_a,
    )
    .await?;

    let a_balance =
        private_token_balance(&wallet_a, &token_artifact, token_address, account_a).await?;
    let b_balance =
        private_token_balance(&wallet_b, &token_artifact, token_address, account_b).await?;

    println!("Token address:      {token_address}");
    println!("PXE A account:      {account_a}");
    println!("PXE B account:      {account_b}");
    println!("PXE A balance:      {a_balance}");
    println!("PXE B balance:      {b_balance}");

    Ok(())
}
