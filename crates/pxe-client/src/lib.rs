//! PXE (Private eXecution Environment) client for the Aztec Rust SDK.
//!
//! Provides the [`Pxe`] trait and [`HttpPxeClient`] for connecting to
//! Aztec PXE nodes over JSON-RPC.

pub mod pxe;
pub use pxe::*;
