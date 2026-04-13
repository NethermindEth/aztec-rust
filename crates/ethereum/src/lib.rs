//! Ethereum integration and L1-L2 messaging for aztec-rs.
//!
//! Provides:
//! - Core messaging types (`L1Actor`, `L2Actor`, `L1ToL2Message`)
//! - Claim types for bridged tokens (`L2Claim`, `L2AmountClaim`)
//! - Cross-chain utilities (`is_l1_to_l2_message_ready`, `wait_for_l1_to_l2_message_ready`)
//! - L1 client for Inbox/Outbox contract interaction (`send_l1_to_l2_message`)

pub mod cross_chain;
pub mod l1_client;
pub mod messaging;
