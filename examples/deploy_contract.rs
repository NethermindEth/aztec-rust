//! Deploy a contract against the local network, then verify wallet and node state.

#![allow(clippy::print_stdout, clippy::wildcard_imports)]

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
    let deploy_result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(next_unique_salt()),
                ..Default::default()
            },
            SendOptions {
                from: owner,
                ..Default::default()
            },
        )
        .await?;

    let address = deploy_result.instance.address;
    let class_id = deploy_result.instance.inner.current_contract_class_id;
    let contract_meta = wallet.get_contract_metadata(address).await?;
    let class_meta = wallet.get_contract_class_metadata(class_id).await?;

    let initial_sum = call_utility_u64(
        &wallet,
        &artifact,
        address,
        "summed_values",
        vec![abi_address(owner)],
        owner,
    )
    .await?;

    let tx_hash = send_call(
        &wallet,
        build_call(
            &artifact,
            address,
            "increment_public_value",
            vec![abi_address(owner), AbiValue::Integer(84)],
        ),
        owner,
    )
    .await?;

    let public_value =
        read_public_u128(&wallet, address, derive_storage_slot_in_map(2, &owner)).await?;

    println!("Contract address:   {address}");
    println!("Deploy tx hash:     {tx_hash}");
    println!("Class ID:           {class_id}");
    println!("Initial sum:        {initial_sum}");
    println!("Public value:       {public_value}");
    println!("Class registered:   {}", class_meta.is_artifact_registered);
    println!(
        "Class published:    {}",
        class_meta.is_contract_class_publicly_registered
    );
    println!(
        "Instance initialized:{}",
        contract_meta.is_contract_initialized
    );

    Ok(())
}
