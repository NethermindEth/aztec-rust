//! Account test group.
//!
//! Tests primarily exercising the `aztec-account` crate.
//!
//! Run only this group:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test account -- --ignored --nocapture
//! ```

#[path = "common/mod.rs"]
pub mod common;

#[path = "account/e2e_account_contracts.rs"]
mod e2e_account_contracts;
#[path = "account/e2e_multiple_accounts_1_enc_key.rs"]
mod e2e_multiple_accounts_1_enc_key;
