//! Create a minimal embedded wallet against the local Aztec network.

#![allow(clippy::print_stdout)]

mod common;

use common::*;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let Some((wallet, account)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return Err(aztec_rs::Error::InvalidData(format!(
            "node not reachable at {}",
            node_url()
        )));
    };

    let chain = wallet.get_chain_info().await?;
    let accounts = wallet.get_accounts().await?;
    let registered_accounts = wallet.pxe().get_registered_accounts().await?;
    let senders = wallet.pxe().get_senders().await?;
    let header = wallet.pxe().get_synced_block_header().await?;

    println!("Wallet account:     {account}");
    println!("Chain ID:           {}", chain.chain_id);
    println!("Version:            {}", chain.version);
    println!(
        "Managed accounts:   {}",
        accounts
            .iter()
            .map(|entry| format!("{}={}", entry.alias, entry.item))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("PXE accounts:       {}", registered_accounts.len());
    println!("PXE senders:        {}", senders.len());
    println!(
        "Synced header keys: {}",
        header
            .data
            .as_object()
            .map(|obj| obj.keys().cloned().collect::<Vec<_>>().join(", "))
            .unwrap_or_else(|| "<opaque>".to_owned())
    );

    Ok(())
}
