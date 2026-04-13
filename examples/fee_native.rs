//! Send a transaction with an explicit native fee payload.

#![allow(clippy::print_stdout, clippy::wildcard_imports)]

mod common;

use common::*;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let Some((wallet, alice)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return Err(aztec_rs::Error::InvalidData(format!(
            "node not reachable at {}",
            node_url()
        )));
    };
    let bob = imported_complete_address(TEST_ACCOUNT_1).address;

    let (token_address, token_artifact, _) = deploy_token(&wallet, alice, 0).await?;
    send_token_method(
        &wallet,
        &token_artifact,
        token_address,
        "mint_to_public",
        vec![AbiValue::Field(Fr::from(alice)), AbiValue::Integer(1_000)],
        alice,
    )
    .await?;

    let fee_payload = NativeFeePaymentMethod::new(alice)
        .get_fee_execution_payload()
        .await?;
    let tx_hash = wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &token_artifact,
                    token_address,
                    "transfer_in_public",
                    vec![
                        AbiValue::Field(Fr::from(alice)),
                        AbiValue::Field(Fr::from(bob)),
                        AbiValue::Integer(10),
                        AbiValue::Integer(0),
                    ],
                )],
                ..Default::default()
            },
            SendOptions {
                from: alice,
                fee_execution_payload: Some(fee_payload),
                gas_settings: Some(GasSettings::default()),
                ..Default::default()
            },
        )
        .await?
        .tx_hash;

    println!("Token address:      {token_address}");
    println!("Alice:              {alice}");
    println!("Bob:                {bob}");
    println!("Tx hash:            {tx_hash}");
    println!(
        "Bob public balance: {}",
        public_balance(&wallet, token_address, &bob).await?
    );

    Ok(())
}
