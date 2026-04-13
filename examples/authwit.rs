//! Create, validate, and consume an auth witness.

#![allow(clippy::print_stdout, clippy::wildcard_imports)]

mod common;

use common::*;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let Some((wallet, owner)) = create_wallet(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1]).await else {
        return Err(aztec_rs::Error::InvalidData(format!(
            "node not reachable at {}",
            node_url()
        )));
    };

    let auth_artifact = load_auth_wit_test_artifact();
    let proxy_artifact = load_generic_proxy_artifact();
    let (auth_address, _, _) =
        deploy_contract(&*wallet, auth_artifact.clone(), vec![], owner).await?;
    let (proxy_address, _, _) =
        deploy_contract(&*wallet, proxy_artifact.clone(), vec![], owner).await?;

    let inner_hash = compute_inner_auth_wit_hash(&[Fr::from_hex("0xdead")?, next_unique_salt()]);
    let intent = MessageHashOrIntent::InnerHash {
        consumer: auth_address,
        inner_hash,
    };
    let witness = wallet.create_auth_wit(owner, intent.clone()).await?;
    let before = lookup_validity(&*wallet, &owner, &intent, &witness).await?;

    let consume_action = build_call(
        &auth_artifact,
        auth_address,
        "consume",
        vec![abi_address(owner), AbiValue::Field(inner_hash)],
    );
    let tx_hash = wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![build_proxy_call(
                    &proxy_artifact,
                    proxy_address,
                    &consume_action,
                )],
                ..Default::default()
            },
            SendOptions {
                from: owner,
                auth_witnesses: vec![witness.clone()],
                ..Default::default()
            },
        )
        .await?
        .tx_hash;
    let after = lookup_validity(&*wallet, &owner, &intent, &witness).await?;

    println!("Auth contract:      {auth_address}");
    println!("Proxy contract:     {proxy_address}");
    println!("Witness hash:       {}", witness.request_hash);
    println!("Validity before:    {before:?}");
    println!("Consume tx hash:    {tx_hash}");
    println!("Validity after:     {after:?}");

    Ok(())
}
