//! Inspect and compare `DeployOptions`.

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

    let artifact = load_stateful_test_artifact();
    let deploy = Contract::deploy(
        &wallet,
        artifact.clone(),
        vec![abi_address(owner), AbiValue::Field(Fr::from(42u64))],
        None,
    )?;

    let standard = DeployOptions {
        contract_address_salt: Some(next_unique_salt()),
        from: Some(owner),
        ..Default::default()
    };
    let universal = DeployOptions {
        contract_address_salt: Some(next_unique_salt()),
        universal_deploy: true,
        ..Default::default()
    };
    let standard_instance = deploy.get_instance(&standard)?;
    let universal_instance = deploy.get_instance(&universal)?;
    let preview_payload = deploy
        .request(&DeployOptions {
            contract_address_salt: Some(next_unique_salt()),
            universal_deploy: true,
            skip_registration: true,
            ..Default::default()
        })
        .await?;
    let sent = deploy
        .send(
            &universal,
            SendOptions {
                from: owner,
                ..Default::default()
            },
        )
        .await?;

    println!("Standard address:   {}", standard_instance.address);
    println!("Universal address:  {}", universal_instance.address);
    println!("Preview call count: {}", preview_payload.calls.len());
    println!("Sent address:       {}", sent.instance.address);
    println!("Sent tx hash:       {}", sent.send_result.tx_hash);

    Ok(())
}
