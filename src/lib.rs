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
pub mod abi {
    pub use aztec_core::abi::*;
}
/// Account abstraction: traits, account manager, and deployment.
pub mod account {
    pub use aztec_account::account::*;
    pub use aztec_account::entrypoint;
    pub use aztec_account::meta_payment;
    pub use aztec_account::schnorr;
    pub use aztec_account::signerless;
    pub use aztec_account::AccountEntrypointMetaPaymentMethod;
    pub use aztec_account::DefaultAccountEntrypoint;
    pub use aztec_account::DefaultMultiCallEntrypoint;
    pub use aztec_account::SchnorrAccountContract;
    pub use aztec_account::SignerlessAccount;
    pub use aztec_account::SingleAccountProvider;
}
/// Authorization witness types and helpers.
pub mod authorization {
    pub use aztec_account::authorization::*;
}
/// Authwit interaction helpers (public authwit, validity checking).
pub mod authwit {
    pub use aztec_contract::authwit::*;
}
/// Contract handles and function interactions.
pub mod contract {
    pub use aztec_contract::contract::*;
}
/// Contract deployment helpers and deployer builder.
pub mod deployment {
    pub use aztec_contract::deployment::*;
}
/// Crate-level error types.
pub mod error {
    pub use aztec_core::error::*;
}
/// Public and private event types and decoding.
pub mod events {
    pub use aztec_contract::events::*;
}
/// Protocol contract addresses and constants.
pub mod constants {
    pub use aztec_core::constants::*;
}
/// Cryptographic primitives and key derivation.
pub mod crypto {
    pub use aztec_crypto::*;
}
/// Poseidon2 hash functions and authwit hash computation.
pub mod hash {
    pub use aztec_core::hash::*;
}
/// Gas and fee payment types.
pub mod fee {
    pub use aztec_core::fee::*;
    pub use aztec_fee::*;
}
/// L1-L2 messaging helpers.
pub use aztec_ethereum::messaging;
/// Node client, readiness polling, and receipt waiting.
pub mod node {
    pub use aztec_node_client::node::*;
}
/// PXE client, readiness polling, and PXE trait.
pub mod pxe {
    pub use aztec_pxe_client::pxe::*;
}
/// Embedded PXE implementation (in-process, client-side).
pub mod embedded_pxe {
    pub use aztec_pxe::*;
}
/// Transaction types, receipts, statuses, and execution payloads.
pub mod tx {
    pub use aztec_core::tx::*;
}
/// Core field, address, key, and contract instance types.
pub mod types {
    pub use aztec_core::types::*;
}
/// Wallet trait, `BaseWallet`, and account provider abstractions.
pub mod wallet {
    pub use aztec_wallet::account_provider::*;
    pub use aztec_wallet::base_wallet::*;
    pub use aztec_wallet::wallet::*;
}

pub use error::Error;
