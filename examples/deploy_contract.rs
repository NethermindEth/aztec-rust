//! Example: Contract deployment workflow.
//!
//! Demonstrates how to use `ContractDeployer` to prepare and deploy a
//! contract using a real wallet connected to a live Aztec sandbox.
//!
//! Run with:
//! ```bash
//! AZTEC_PXE_URL=http://localhost:8080 AZTEC_NODE_URL=http://localhost:8080 \
//!     cargo run --example deploy_contract
//! ```

#![allow(clippy::print_stdout)]

use aztec_rs::abi::{AbiValue, ContractArtifact};
use aztec_rs::account::{AccountManager, SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::deployment::{ContractDeployer, DeployOptions};
use aztec_rs::node::{create_aztec_node_client, wait_for_node, AztecNode};
use aztec_rs::types::{CompleteAddress, Fr};
use aztec_rs::wallet::create_wallet_from_urls;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let pxe_url =
        std::env::var("AZTEC_PXE_URL").unwrap_or_else(|_| "http://localhost:8080".into());
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

    // -- Create a wallet -------------------------------------------------------

    let secret_key = Fr::from(0xcafe_u64);
    let bootstrap_wallet = create_wallet_from_urls(
        &pxe_url,
        &node_url,
        SingleAccountProvider::new(
            CompleteAddress::default(),
            Box::new(SchnorrAccountContract::new(secret_key)),
            "bootstrap",
        ),
    );
    let manager = AccountManager::create(
        bootstrap_wallet,
        secret_key,
        Box::new(SchnorrAccountContract::new(secret_key)),
        Some(Fr::from(1u64)),
    )
    .await?;

    let complete_address = manager.complete_address().await?;
    let wallet = create_wallet_from_urls(
        &pxe_url,
        &node_url,
        SingleAccountProvider::new(
            complete_address,
            Box::new(SchnorrAccountContract::new(secret_key)),
            "main",
        ),
    );

    // -- Load a contract artifact ----------------------------------------------

    let artifact_json = include_str!("../fixtures/token_contract.json");
    let artifact = ContractArtifact::from_json(artifact_json)?;
    println!("Loaded artifact: {}", artifact.name);
    println!(
        "  functions: {}",
        artifact
            .functions
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Find the constructor and display its parameters.
    let constructor = artifact.find_function("constructor")?;
    println!("\nConstructor: {}", constructor.name);
    println!("  is_initializer: {}", constructor.is_initializer);
    println!(
        "  parameters: {}",
        constructor
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    // -- Build a deployer ------------------------------------------------------

    let deployer = ContractDeployer::new(artifact, &wallet).with_constructor_name("constructor");
    println!("\nDeployer: {deployer:?}");

    // Create a deploy method with constructor arguments.
    let deploy_method = deployer.deploy(vec![
        AbiValue::Field(Fr::from(1u64)),       // admin
        AbiValue::String("TestToken".into()),  // name
        AbiValue::String("TT".into()),         // symbol
        AbiValue::Integer(18),                 // decimals
    ])?;
    println!("Deploy method: {deploy_method:?}");

    // Get the computed contract instance.
    let opts = DeployOptions {
        contract_address_salt: Some(Fr::from(42u64)),
        universal_deploy: true,
        ..DeployOptions::default()
    };
    let instance = deploy_method.get_instance(&opts)?;
    println!("\nContract instance:");
    println!("  address:  {}", instance.address);
    println!("  version:  {}", instance.inner.version);
    println!("  salt:     {}", instance.inner.salt);
    println!("  class_id: {}", instance.inner.current_contract_class_id);

    // Build the full deployment payload.
    let opts_with_skip = DeployOptions {
        contract_address_salt: Some(Fr::from(42u64)),
        universal_deploy: true,
        skip_registration: true,
        ..DeployOptions::default()
    };
    let payload = deploy_method.request(&opts_with_skip).await?;
    println!("\nDeployment payload built successfully.");
    println!("  calls: {}", payload.calls.len());

    println!("\nDone.");

    Ok(())
}
