//! Rust SDK for the Aztec Network.
//!
//! `aztec-rs` provides a client library for interacting with Aztec nodes,
//! managing wallets and accounts, deploying contracts, and sending
//! transactions. The design mirrors the upstream `aztec.js` package.
//!
//! # Quick Start
//!
//! ```no_run
//! use aztec_rs::node::{create_aztec_node_client, wait_for_node, AztecNode};
//!
//! # async fn example() -> Result<(), aztec_rs::Error> {
//! let node = create_aztec_node_client("http://localhost:8080");
//! let info = wait_for_node(&node).await?;
//! println!("Connected to node v{}", info.node_version);
//! let block = node.get_block_number().await?;
//! println!("Current block: {block}");
//! # Ok(())
//! # }
//! ```

/// ABI types, selectors, and contract artifact loading.
pub mod abi;
/// Account abstraction: traits, account manager, and deployment.
pub mod account;
/// Authorization witness types and helpers.
pub mod authorization;
/// Contract handles and function interactions.
pub mod contract;
/// Contract deployment helpers and deployer builder.
pub mod deployment;
/// Crate-level error types.
pub mod error;
/// Public and private event types and decoding.
pub mod events;
/// Gas and fee payment types.
pub mod fee;
/// L1-L2 messaging helpers.
pub mod messaging;
/// Node client, readiness polling, and receipt waiting.
pub mod node;
mod rpc;
/// Transaction types, receipts, statuses, and execution payloads.
pub mod tx;
/// Core field, address, key, and contract instance types.
pub mod types;
/// Wallet trait and mock implementation.
pub mod wallet;

pub use error::Error;
