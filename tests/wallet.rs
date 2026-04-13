//! Wallet test group.
//!
//! Tests primarily exercising the `aztec-wallet` crate.
//!
//! Run only this group:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test wallet -- --ignored --nocapture
//! ```

#[path = "common/mod.rs"]
pub mod common;

#[path = "wallet/e2e_double_spend.rs"]
mod e2e_double_spend;
