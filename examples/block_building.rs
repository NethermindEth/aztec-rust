//! Send multiple transactions and inspect the resulting block state.

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

    let tx1 = send_call(
        &wallet,
        build_call(
            &artifact,
            contract_address,
            "increment_public_value_no_init_check",
            vec![abi_address(owner), AbiValue::Field(Fr::from(1u64))],
        ),
        owner,
    )
    .await?;
    let tx2 = send_call(
        &wallet,
        build_call(
            &artifact,
            contract_address,
            "increment_public_value_no_init_check",
            vec![abi_address(owner), AbiValue::Field(Fr::from(2u64))],
        ),
        owner,
    )
    .await?;

    let receipt1 = wallet.node().get_tx_receipt(&tx1).await?;
    let receipt2 = wallet.node().get_tx_receipt(&tx2).await?;
    let final_value = read_public_u128(
        &wallet,
        contract_address,
        derive_storage_slot_in_map(2, &owner),
    )
    .await?;

    println!("Contract address:   {contract_address}");
    println!("Tx1 block:          {:?}", receipt1.block_number);
    println!("Tx2 block:          {:?}", receipt2.block_number);
    println!("Final public value: {final_value}");

    Ok(())
}
