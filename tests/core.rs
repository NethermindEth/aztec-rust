//! Core test group.
//!
//! Tests primarily exercising the `aztec-core` crate.
//!
//! Run only this group:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test core -- --ignored --nocapture
//! ```

#[path = "common/mod.rs"]
pub mod common;

#[path = "core/e2e_abi_types.rs"]
mod e2e_abi_types;
#[path = "core/e2e_expiration_timestamp.rs"]
mod e2e_expiration_timestamp;
#[path = "core/e2e_phase_check.rs"]
mod e2e_phase_check;
