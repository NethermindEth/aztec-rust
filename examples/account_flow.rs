//! Example: Account lifecycle workflow.
//!
//! Demonstrates how to use `AccountManager` with `SchnorrAccountContract`
//! to prepare account creation and deployment against a live Aztec sandbox.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo run --example account_flow
//! ```

#![allow(clippy::print_stdout)]

use aztec_rs::account::{
    AccountManager, DeployAccountOptions, SchnorrAccountContract, SingleAccountProvider,
};
use aztec_rs::node::{create_aztec_node_client, wait_for_node, AztecNode};
use aztec_rs::types::{CompleteAddress, Fr};
use aztec_rs::wallet::create_embedded_wallet;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let node_url =
        std::env::var("AZTEC_NODE_URL").unwrap_or_else(|_| "http://localhost:8080".into());

    // -- Connect to the network ------------------------------------------------

    println!("Connecting to node at {node_url}...");
    let node = create_aztec_node_client(&node_url);
    let info = wait_for_node(&node).await?;
    println!(
        "Node ready: version={}, chain={}, block={}",
        info.node_version,
        info.l1_chain_id,
        node.get_block_number().await?
    );

    // -- Create the Schnorr account contract -----------------------------------

    let secret_key = Fr::from(12345u64);
    let salt = Fr::from(99u64);
    let account_contract = SchnorrAccountContract::new(secret_key);
    println!("\nAccount contract: SchnorrAccount");
    println!("Secret key:       {secret_key}");
    println!(
        "Signing pub key:  ({}, {})",
        account_contract.signing_public_key().x,
        account_contract.signing_public_key().y,
    );

    // -- Bootstrap wallet to derive the address --------------------------------

    let bootstrap_wallet = create_embedded_wallet(
        &node_url,
        SingleAccountProvider::new(
            CompleteAddress::default(),
            Box::new(SchnorrAccountContract::new(secret_key)),
            "bootstrap",
        ),
    )
    .await?;

    let manager = AccountManager::create(
        bootstrap_wallet,
        secret_key,
        Box::new(account_contract),
        Some(salt),
    )
    .await?;

    println!("\nAccountManager created:");
    println!("  salt:            {}", manager.salt());
    println!("  secret_key:      {}", manager.secret_key());
    println!("  has_initializer: {}", manager.has_initializer());

    // -- Access the contract instance ------------------------------------------

    let instance = manager.instance();
    println!("\nContract instance:");
    println!("  address: {}", instance.address);
    println!("  version: {}", instance.inner.version);
    println!("  salt:    {}", instance.inner.salt);

    // -- Complete address (key derivation + address computation) ----------------

    let complete_addr = manager.complete_address().await?;
    println!("\nComplete address: {}", complete_addr.address);

    // -- Build a deployment payload --------------------------------------------

    let deploy = manager.deploy_method().await?;
    println!(
        "\nDeploy method created for instance: {}",
        deploy.instance().address
    );
    let opts = DeployAccountOptions {
        skip_registration: true,
        ..Default::default()
    };
    let payload = deploy.request(&opts).await?;
    println!(
        "Deployment payload built with {} call(s).",
        payload.calls.len()
    );

    // -- Verify deterministic address ------------------------------------------

    let contract = SchnorrAccountContract::new(secret_key);
    let pre_addr =
        aztec_rs::account::get_account_contract_address(&contract, secret_key, salt).await?;
    println!("\nPre-deployment address: {pre_addr}");
    println!("Manager address:        {}", manager.address());
    assert_eq!(pre_addr, manager.address());
    println!("Addresses match!");

    println!("\nDone.");

    Ok(())
}
