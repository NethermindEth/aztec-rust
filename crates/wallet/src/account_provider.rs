//! Account provider abstraction for wallet implementations.
//!
//! The [`AccountProvider`] trait decouples wallet implementations from specific
//! account types. It provides a bridge between the account abstraction layer
//! (which knows about secret keys, entrypoints, and signing) and the wallet
//! layer (which coordinates PXE and node interactions).

use async_trait::async_trait;

use crate::error::Error;
use crate::fee::GasSettings;
use crate::pxe::TxExecutionRequest;
use crate::tx::{AuthWitness, ExecutionPayload};
use crate::types::{AztecAddress, CompleteAddress};
use crate::wallet::{Aliased, ChainInfo, MessageHashOrIntent};

/// Provides account operations needed by [`BaseWallet`](crate::BaseWallet).
///
/// This is the Rust equivalent of the abstract `getAccountFromAddress()`
/// in the TS BaseWallet. Different wallet backends (embedded, CLI, extension)
/// implement this to provide their own account lookup and transaction creation
/// strategy.
///
/// The trait returns PXE-level [`TxExecutionRequest`] (opaque JSON) directly,
/// handling the conversion from the account's structured request internally.
/// This avoids a circular dependency between the wallet and account crates.
#[async_trait]
pub trait AccountProvider: Send + Sync {
    /// Create a transaction execution request for the given account.
    ///
    /// This processes the execution payload through the account's entrypoint,
    /// adding authentication and gas handling, then serializes the result
    /// into the PXE's opaque request format.
    async fn create_tx_execution_request(
        &self,
        from: &AztecAddress,
        exec: ExecutionPayload,
        gas_settings: GasSettings,
        chain_info: &ChainInfo,
        fee_payer: Option<AztecAddress>,
    ) -> Result<TxExecutionRequest, Error>;

    /// Create an authorization witness for the given account.
    async fn create_auth_wit(
        &self,
        from: &AztecAddress,
        intent: MessageHashOrIntent,
        chain_info: &ChainInfo,
    ) -> Result<AuthWitness, Error>;

    /// Get the complete address for a managed account, if available.
    async fn get_complete_address(
        &self,
        address: &AztecAddress,
    ) -> Result<Option<CompleteAddress>, Error>;

    /// Get all account addresses managed by this provider.
    async fn get_accounts(&self) -> Result<Vec<Aliased<AztecAddress>>, Error>;
}
