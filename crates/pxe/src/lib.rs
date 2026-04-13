//! Embedded PXE (Private eXecution Environment) runtime for aztec-rs.
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

pub use embedded_pxe::{EmbeddedPxe, EmbeddedPxeConfig};
pub use kernel::{
    BbPrivateKernelProver, BbProverConfig, ChonkProofWithPublicInputs, PrivateExecutionStep,
    PrivateKernelExecutionProver, PrivateKernelOracle, PrivateKernelProver,
    PrivateKernelSimulateOutput, SimulatedKernel,
};
pub use stores::kv::{InMemoryKvStore, KvStore};
pub use stores::{
    AnchorBlockStore, NoteStore, PrivateEventStore, RecipientTaggingStore, SenderTaggingStore,
    SledKvStore,
};
pub use sync::{
    BlockStateSynchronizer, BlockSyncConfig, ContractSyncService, EventService, LogService,
    NoteService, PrivateEventFilterValidator,
};
