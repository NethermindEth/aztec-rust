#![allow(clippy::print_stdout, clippy::unwrap_used)]

use aztec_rs::node::{create_aztec_node_client, wait_for_node, AztecNode};

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let url = std::env::var("AZTEC_NODE_URL").unwrap_or_else(|_| "http://localhost:8080".into());
    println!("Connecting to Aztec node at {url}...");

    let node = create_aztec_node_client(&url);

    let info = wait_for_node(&node).await?;
    println!("Node is ready!");
    println!("  version:          {}", info.node_version);
    println!("  L1 chain ID:      {}", info.l1_chain_id);
    println!("  rollup version:   {}", info.rollup_version);
    println!("  real proofs:      {}", info.real_proofs);
    if let Some(enr) = &info.enr {
        println!("  ENR:              {enr}");
    }

    let block_number = node.get_block_number().await?;
    println!("  block number:     {block_number}");

    Ok(())
}
