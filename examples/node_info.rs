//! Connect to a local Aztec network and print basic node metadata.
//!
//! Run with:
//! ```bash
//! aztec start --local-network
//! cargo run --example node_info
//! ```

#![allow(clippy::print_stdout)]

mod common;

use common::*;

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let url = node_url();
    let node = create_aztec_node_client(&url);
    let info = wait_for_node(&node).await?;

    println!("Node URL:           {url}");
    println!("Node version:       {}", info.node_version);
    println!("L1 chain ID:        {}", info.l1_chain_id);
    println!("Rollup version:     {}", info.rollup_version);
    println!("Real proofs:        {}", info.real_proofs);
    println!("Current block:      {}", node.get_block_number().await?);
    println!(
        "Proven block:       {}",
        node.get_proven_block_number().await?
    );

    if let Some(enr) = info.enr {
        println!("ENR:                {enr}");
    }

    if let Some(inbox) = info
        .l1_contract_addresses
        .get("inboxAddress")
        .and_then(|value| value.as_str())
    {
        println!("L1 inbox:           {inbox}");
    }

    Ok(())
}
