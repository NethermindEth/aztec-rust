#![allow(clippy::print_stdout)]

use aztec_rs::abi::{AbiValue, ContractArtifact};
use aztec_rs::contract::Contract;
use aztec_rs::types::{AztecAddress, Fr};
use aztec_rs::wallet::{ChainInfo, MockWallet, SendOptions, SimulateOptions};

/// A minimal token contract artifact with selectors.
const TOKEN_ARTIFACT: &str = r#"
{
  "name": "TokenContract",
  "functions": [
    {
      "name": "transfer",
      "function_type": "private",
      "is_initializer": false,
      "is_static": false,
      "parameters": [
        { "name": "from", "type": { "kind": "field" } },
        { "name": "to", "type": { "kind": "field" } },
        { "name": "amount", "type": { "kind": "integer", "sign": "unsigned", "width": 64 } }
      ],
      "return_types": [],
      "selector": "0xd6f42325"
    },
    {
      "name": "balance_of",
      "function_type": "utility",
      "is_initializer": false,
      "is_static": true,
      "parameters": [
        { "name": "owner", "type": { "kind": "field" } }
      ],
      "return_types": [
        { "kind": "integer", "sign": "unsigned", "width": 64 }
      ],
      "selector": "0x12345678"
    }
  ]
}
"#;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    // Load a contract artifact from JSON.
    let artifact = ContractArtifact::from_json(TOKEN_ARTIFACT)?;
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

    // Create a mock wallet (replace with a real wallet backend when available).
    let wallet = MockWallet::new(ChainInfo {
        chain_id: Fr::from(31337u64),
        version: Fr::from(1u64),
    });

    // Build a contract handle at a specific address.
    let contract_address = AztecAddress(Fr::from(42u64));
    let contract = Contract::at(contract_address, artifact, wallet);
    let from = AztecAddress(Fr::from(1u64));
    println!("\nContract at: {contract_address}");

    // Simulate a utility call (balance_of).
    let sim_result = contract
        .method("balance_of", vec![AbiValue::Field(Fr::from(1u64))])?
        .simulate(SimulateOptions {
            from,
            ..SimulateOptions::default()
        })
        .await?;
    println!("\nSimulated balance_of:");
    println!("  return_values: {}", sim_result.return_values);
    println!("  gas_used:      {:?}", sim_result.gas_used);

    // Send a transfer transaction.
    let send_result = contract
        .method(
            "transfer",
            vec![
                AbiValue::Field(Fr::from(1u64)),
                AbiValue::Field(Fr::from(2u64)),
                AbiValue::Integer(100),
            ],
        )?
        .send(SendOptions {
            from,
            ..SendOptions::default()
        })
        .await?;
    println!("\nSent transfer:");
    println!("  tx_hash: {}", send_result.tx_hash);

    Ok(())
}
