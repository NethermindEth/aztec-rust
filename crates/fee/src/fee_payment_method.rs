use async_trait::async_trait;
use aztec_core::tx::ExecutionPayload;
use aztec_core::types::AztecAddress;
use aztec_core::Error;

/// Trait that all fee payment strategies implement.
///
/// Each implementation produces an [`ExecutionPayload`] containing the
/// function calls needed to set up fee payment. This payload is merged
/// with the user's transaction payload before submission.
#[async_trait]
pub trait FeePaymentMethod: Send + Sync {
    /// Returns the address of the asset used for fee payment.
    /// For fee juice methods, this is the FeeJuice protocol contract address.
    async fn get_asset(&self) -> Result<AztecAddress, Error>;

    /// Returns the address of the entity paying the fee.
    async fn get_fee_payer(&self) -> Result<AztecAddress, Error>;

    /// Builds an [`ExecutionPayload`] containing the function calls and
    /// auth witnesses needed to pay the transaction fee.
    async fn get_fee_execution_payload(&self) -> Result<ExecutionPayload, Error>;
}
