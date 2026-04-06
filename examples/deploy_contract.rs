//! Example: Contract deployment workflow.
//!
//! Demonstrates how to use `ContractDeployer` to prepare a contract
//! deployment using a representative fixture artifact. Uses a mock wallet
//! since full deployment (class publication, instance publication, address
//! derivation) is not yet implemented.
//!
//! Run with:
//! ```bash
//! cargo run --example deploy_contract
//! ```

#![allow(clippy::print_stdout)]

use aztec_rs::abi::{AbiValue, ContractArtifact};
use aztec_rs::deployment::{ContractDeployer, DeployOptions};
use aztec_rs::types::Fr;
use aztec_rs::wallet::{ChainInfo, MockWallet};

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    // Load a contract artifact from a fixture file.
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

    // Create a mock wallet.
    let wallet = MockWallet::new(ChainInfo {
        chain_id: Fr::from(31337u64),
        version: Fr::from(1u64),
    });

    // Build a deployer with a specific salt for deterministic address computation.
    let deployer = ContractDeployer::new(artifact, &wallet).with_constructor_name("constructor");
    println!("\nDeployer: {deployer:?}");

    // Create a deploy method with constructor arguments.
    let deploy_method = deployer.deploy(vec![
        AbiValue::Field(Fr::from(1u64)),      // admin
        AbiValue::String("TestToken".into()), // name
        AbiValue::String("TT".into()),        // symbol
        AbiValue::Integer(18),                // decimals
    ])?;
    println!("Deploy method: {deploy_method:?}");

    // Get the computed contract instance (uses placeholder address).
    let opts = DeployOptions {
        contract_address_salt: Some(Fr::from(42u64)),
        ..DeployOptions::default()
    };
    let instance = deploy_method.get_instance(&opts);
    println!("\nContract instance:");
    println!("  address (placeholder): {}", instance.address);
    println!("  version:               {}", instance.inner.version);
    println!("  salt:                  {}", instance.inner.salt);

    // Attempting to build the full deployment payload is expected to fail
    // because address derivation and publication helpers are not yet implemented.
    match deploy_method.request(&opts) {
        Ok(_) => println!("\nDeployment payload built successfully."),
        Err(e) => println!("\nDeployment request (expected deferred): {e}"),
    }

    println!("\nDone. Full deployment requires protocol primitives not yet implemented.");

    Ok(())
}
