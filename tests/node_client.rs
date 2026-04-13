//! Node client test group.
//!
//! Tests primarily exercising the `aztec-node-client` crate.
//!
//! Run only this group:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test node_client -- --ignored --nocapture
//! ```

#[path = "common/mod.rs"]
pub mod common;

#[path = "node_client/e2e_block_building.rs"]
mod e2e_block_building;
