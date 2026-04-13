//! Write and read public storage directly.

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

    let (contract_address, artifact, _) = deploy_contract(
        &wallet,
        load_stateful_test_artifact(),
        vec![abi_address(owner), AbiValue::Field(Fr::from(1u64))],
        owner,
    )
    .await?;
    let tx_hash = send_call(
        &wallet,
        build_call(
            &artifact,
            contract_address,
            "increment_public_value_no_init_check",
            vec![abi_address(owner), AbiValue::Field(Fr::from(9u64))],
        ),
        owner,
    )
    .await?;

    let slot = derive_storage_slot_in_map(2, &owner);
    let value = read_public_storage(&wallet, contract_address, slot).await?;

    println!("Contract address:   {contract_address}");
    println!("Write tx hash:      {tx_hash}");
    println!("Slot:               {slot}");
    println!("Stored value:       {}", value.to_usize());

    Ok(())
}
