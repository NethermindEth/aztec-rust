use async_trait::async_trait;
use aztec_core::constants::protocol_contract_address;
use aztec_core::tx::ExecutionPayload;
use aztec_core::types::AztecAddress;
use aztec_core::Error;

use crate::fee_payment_method::FeePaymentMethod;

/// Pays transaction fees using the sender's existing Fee Juice balance.
///
/// This is the default fee payment strategy. It doesn't add any extra
/// function calls to the transaction — it simply declares who is paying.
/// The sender must already have sufficient Fee Juice balance.
pub struct NativeFeePaymentMethod {
    /// Address of the account paying fees from its existing balance.
    sender: AztecAddress,
}

impl NativeFeePaymentMethod {
    /// Create a new native fee payment method.
    ///
    /// `sender` is the account that will pay fees from its existing Fee Juice balance.
    pub fn new(sender: AztecAddress) -> Self {
        Self { sender }
    }
}

#[async_trait]
impl FeePaymentMethod for NativeFeePaymentMethod {
    async fn get_asset(&self) -> Result<AztecAddress, Error> {
        Ok(protocol_contract_address::fee_juice())
    }

    async fn get_fee_payer(&self) -> Result<AztecAddress, Error> {
        Ok(self.sender)
    }

    async fn get_fee_execution_payload(&self) -> Result<ExecutionPayload, Error> {
        Ok(ExecutionPayload {
            calls: vec![],
            auth_witnesses: vec![],
            capsules: vec![],
            extra_hashed_args: vec![],
            fee_payer: Some(self.sender),
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use aztec_core::types::Fr;

    #[tokio::test]
    async fn payload_is_empty_except_fee_payer() {
        let sender = AztecAddress(Fr::from(10u64));
        let method = NativeFeePaymentMethod::new(sender);
        let payload = method.get_fee_execution_payload().await.expect("payload");

        assert!(payload.calls.is_empty());
        assert!(payload.auth_witnesses.is_empty());
        assert!(payload.capsules.is_empty());
        assert!(payload.extra_hashed_args.is_empty());
        assert_eq!(payload.fee_payer, Some(sender));
    }

    #[tokio::test]
    async fn asset_is_fee_juice() {
        let sender = AztecAddress(Fr::from(10u64));
        let method = NativeFeePaymentMethod::new(sender);
        assert_eq!(
            method.get_asset().await.expect("asset"),
            protocol_contract_address::fee_juice()
        );
    }

    #[tokio::test]
    async fn fee_payer_is_sender() {
        let sender = AztecAddress(Fr::from(10u64));
        let method = NativeFeePaymentMethod::new(sender);
        assert_eq!(method.get_fee_payer().await.expect("fee payer"), sender);
    }
}
