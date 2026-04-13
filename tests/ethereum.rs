//! Ethereum / cross-chain messaging test group.
//!
//! Tests primarily exercising the `aztec-ethereum` crate (L1 ↔ L2 inbox/outbox,
//! token-bridge failure + happy paths, L1 client RPC).
//!
//! Run only this group:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test ethereum -- --ignored --nocapture
//! ```

#[path = "common/mod.rs"]
pub mod common;

#[path = "ethereum/e2e_cross_chain_l1_to_l2.rs"]
mod e2e_cross_chain_l1_to_l2;
#[path = "ethereum/e2e_cross_chain_l2_to_l1.rs"]
mod e2e_cross_chain_l2_to_l1;
#[path = "ethereum/e2e_cross_chain_token_bridge_failure_cases.rs"]
mod e2e_cross_chain_token_bridge_failure_cases;
#[path = "ethereum/e2e_cross_chain_token_bridge_private.rs"]
mod e2e_cross_chain_token_bridge_private;
#[path = "ethereum/e2e_cross_chain_token_bridge_public.rs"]
mod e2e_cross_chain_token_bridge_public;
