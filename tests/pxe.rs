//! PXE test group.
//!
//! Tests primarily exercising the `aztec-pxe` crate.
//!
//! Run only this group:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test pxe -- --ignored --nocapture
//! ```

#[path = "common/mod.rs"]
pub mod common;

#[path = "pxe/e2e_2_pxes.rs"]
mod e2e_2_pxes;
#[path = "pxe/e2e_bench.rs"]
mod e2e_bench;
#[path = "pxe/e2e_kernelless_simulation.rs"]
mod e2e_kernelless_simulation;
#[path = "pxe/e2e_note_getter.rs"]
mod e2e_note_getter;
#[path = "pxe/e2e_offchain_effects.rs"]
mod e2e_offchain_effects;
#[path = "pxe/e2e_partial_notes.rs"]
mod e2e_partial_notes;
#[path = "pxe/e2e_pending_note_hashes.rs"]
mod e2e_pending_note_hashes;
#[path = "pxe/e2e_pruned_blocks.rs"]
mod e2e_pruned_blocks;
#[path = "pxe/e2e_scope_isolation.rs"]
mod e2e_scope_isolation;
