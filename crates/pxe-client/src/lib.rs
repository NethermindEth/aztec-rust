//! PXE (Private eXecution Environment) trait and shared types for aztec-rs.
//!
//! Provides the [`Pxe`] trait and supporting types for PXE implementations.
//! The primary implementation is [`aztec_pxe::EmbeddedPxe`](../aztec_pxe/struct.EmbeddedPxe.html).

pub mod pxe;
pub use pxe::*;
