//! Compare simulation, profiling, and sending for the same call.

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

    let (address, artifact, _) = deploy_contract(
        &wallet,
        load_stateful_test_artifact(),
        vec![abi_address(owner), AbiValue::Field(Fr::from(1u64))],
        owner,
    )
    .await?;

    let call = build_call(
        &artifact,
        address,
        "increment_public_value_no_init_check",
        vec![abi_address(owner), AbiValue::Field(Fr::from(5u64))],
    );
    let payload = ExecutionPayload {
        calls: vec![call],
        ..Default::default()
    };

    let sim = wallet
        .simulate_tx(
            payload.clone(),
            SimulateOptions {
                from: owner,
                estimate_gas: true,
                ..Default::default()
            },
        )
        .await?;
    let gas_limits = get_gas_limits(&sim, None);

    let profile = wallet
        .profile_tx(
            payload.clone(),
            ProfileOptions {
                from: owner,
                profile_mode: Some(ProfileMode::Full),
                ..Default::default()
            },
        )
        .await?;

    let tx_hash = wallet
        .send_tx(
            payload,
            SendOptions {
                from: owner,
                gas_settings: Some(GasSettings {
                    gas_limits: Some(gas_limits.gas_limits.clone()),
                    teardown_gas_limits: Some(gas_limits.teardown_gas_limits.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await?
        .tx_hash;

    let updated_value =
        read_public_u128(&wallet, address, derive_storage_slot_in_map(2, &owner)).await?;

    println!("Contract address:   {address}");
    println!("Sim return values:  {}", sim.return_values);
    println!(
        "Suggested gas:      da={} l2={}",
        gas_limits.gas_limits.da_gas, gas_limits.gas_limits.l2_gas
    );
    println!("Profile payload:    {}", profile.profile_data);
    println!("Sent tx hash:       {tx_hash}");
    println!("Updated value:      {updated_value}");

    Ok(())
}
