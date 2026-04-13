//! Deploy a fresh Schnorr account and then use it with its own wallet.

#![allow(clippy::print_stdout)]

mod common;

use common::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let Some((sponsor_wallet, sponsor)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return Err(aztec_rs::Error::InvalidData(format!(
            "node not reachable at {}",
            node_url()
        )));
    };

    let secret = Fr::random();
    let sponsor_wallet = Arc::new(sponsor_wallet);
    let manager = AccountManager::create(
        Arc::clone(&sponsor_wallet),
        secret,
        Box::new(SchnorrAccountContract::new(secret)),
        Some(next_unique_salt()),
    )
    .await?;
    let account_address = manager.address();
    let complete = manager.complete_address().await?;
    let instance = manager.instance().clone();

    let compiled_account = load_schnorr_account_artifact();
    let class_id = instance.inner.current_contract_class_id;
    sponsor_wallet
        .pxe()
        .contract_store()
        .add_artifact(&class_id, &compiled_account)
        .await?;
    sponsor_wallet
        .pxe()
        .contract_store()
        .add_instance(&instance)
        .await?;
    sponsor_wallet
        .pxe()
        .key_store()
        .add_account(&secret)
        .await?;
    sponsor_wallet.pxe().address_store().add(&complete).await?;
    seed_signing_key_note(
        sponsor_wallet.pxe(),
        &SchnorrAccountContract::new(secret),
        account_address,
        2,
    )
    .await;

    let deploy_result = manager
        .deploy_method()
        .await?
        .send(
            &DeployAccountOptions {
                from: Some(sponsor),
                ..Default::default()
            },
            SendOptions {
                from: sponsor,
                additional_scopes: vec![account_address],
                ..Default::default()
            },
        )
        .await?;

    let (account_wallet, _, _, _) =
        setup_registered_schnorr_wallet(secret, complete.clone(), instance, "generated").await?;
    let managed_accounts = account_wallet.get_accounts().await?;
    let pxe_accounts = account_wallet.pxe().get_registered_accounts().await?;
    let auth_wit = account_wallet
        .create_auth_wit(
            account_address,
            MessageHashOrIntent::Hash {
                hash: Fr::from(123u64),
            },
        )
        .await?;

    println!("Sponsor:            {sponsor}");
    println!("Account address:    {}", complete.address);
    println!("Deploy tx hash:     {}", deploy_result.send_result.tx_hash);
    println!(
        "Managed accounts:   {}",
        managed_accounts
            .iter()
            .map(|entry| format!("{}={}", entry.alias, entry.item))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("Auth witness hash:  {}", auth_wit.request_hash);
    println!("PXE accounts:       {}", pxe_accounts.len());

    Ok(())
}
