//! Embedded PXE (Private eXecution Environment) for the Aztec Rust SDK.
//!
//! This crate provides an in-process PXE implementation for Aztec v4.x,
//! where PXE runs client-side. It implements
//! the [`Pxe`](aztec_pxe_client::Pxe) trait using local stores and the
//! Aztec node's `node_*` RPC methods.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────┐
//! │  BaseWallet              │
//! │  ┌───────────────────┐  │
//! │  │ EmbeddedPxe        │  │
//! │  │  - local stores    │  │
//! │  │  - ACVM executor   │  │
//! │  └──────────┬────────┘  │
//! │             │            │
//! │  ┌──────────▼─────────┐ │
//! │  │ HttpNodeClient      │ │
//! │  └────────────────────┘ │
//! └─────────────────────────┘
//! ```

pub mod embedded_pxe;
pub mod execution;
pub mod kernel;
pub mod stores;
pub mod sync;

pub use embedded_pxe::EmbeddedPxe;
pub use stores::kv::{InMemoryKvStore, KvStore};
