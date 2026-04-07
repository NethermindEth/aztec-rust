//! Example: Contract deployment workflow.
//!
//! Demonstrates how to use `ContractDeployer` to prepare a contract
//! deployment using a representative fixture artifact. Uses a mock wallet
//! since the real PXE node is not available.
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
    match deploy_method.request(&opts_with_skip).await {
        Ok(payload) => {
            println!("\nDeployment payload built successfully.");
            println!("  calls: {}", payload.calls.len());
        }
        Err(e) => println!("\nDeployment request failed: {e}"),
    }

    println!("\nDone.");

    Ok(())
}
