//! Crypto test group.
//!
//! Tests primarily exercising the `aztec-crypto` crate.
//!
//! Run only this group:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test crypto -- --ignored --nocapture
//! ```

#[path = "common/mod.rs"]
pub mod common;

#[path = "crypto/e2e_keys.rs"]
mod e2e_keys;
