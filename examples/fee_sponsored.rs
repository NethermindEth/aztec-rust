//! Build a sponsored fee payload against a local network.

#![allow(clippy::print_stdout)]

mod common;

use common::*;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let Some((wallet, owner)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return Err(aztec_rs::Error::InvalidData(format!(
            "node not reachable at {}",
            node_url()
        )));
    };

    let Some(sponsored_artifact) = load_sponsored_fpc_artifact() else {
        println!("SponsoredFPC artifact not found. Add it under fixtures/ or aztec-packages.");
        return Ok(());
    };

    let (sponsor_address, _, _) =
        deploy_contract(&wallet, sponsored_artifact, vec![], owner).await?;
    let payload = SponsoredFeePaymentMethod::new(sponsor_address)
        .get_fee_execution_payload()
        .await?;

    println!("Sponsor contract:   {sponsor_address}");
    println!("Call count:         {}", payload.calls.len());
    println!("Fee payer:          {:?}", payload.fee_payer);
    println!("Note:               pre-fund the sponsor with FeeJuice before using this payload to send txs");

    Ok(())
}
