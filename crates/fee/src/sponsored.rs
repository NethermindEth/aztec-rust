use async_trait::async_trait;
use aztec_core::abi::{FunctionSelector, FunctionType};
use aztec_core::tx::{ExecutionPayload, FunctionCall};
use aztec_core::types::AztecAddress;
use aztec_core::Error;

use crate::fee_payment_method::FeePaymentMethod;

/// A fee payment method where a sponsor contract pays the fee unconditionally.
///
/// This is the simplest strategy — useful for testing, development, and
/// gasless transaction experiences. The sponsor contract must be pre-funded
/// with Fee Juice and expose a `sponsor_unconditionally()` private function.
pub struct SponsoredFeePaymentMethod {
    /// Address of the sponsor contract that will pay fees.
    payment_contract: AztecAddress,
}

impl SponsoredFeePaymentMethod {
    /// Create a new sponsored fee payment method.
    ///
    /// `payment_contract` is the address of the sponsor contract that will pay fees.
    pub fn new(payment_contract: AztecAddress) -> Self {
        Self { payment_contract }
    }
}

#[async_trait]
impl FeePaymentMethod for SponsoredFeePaymentMethod {
    async fn get_asset(&self) -> Result<AztecAddress, Error> {
        Err(Error::InvalidData(
            "SponsoredFeePaymentMethod does not have an associated asset".into(),
        ))
    }

    async fn get_fee_payer(&self) -> Result<AztecAddress, Error> {
        Ok(self.payment_contract)
    }

    async fn get_fee_execution_payload(&self) -> Result<ExecutionPayload, Error> {
        let call = FunctionCall {
            to: self.payment_contract,
            selector: FunctionSelector::from_signature("sponsor_unconditionally()"),
            args: vec![],
            function_type: FunctionType::Private,
            is_static: false,
            hide_msg_sender: false,
        };

        Ok(ExecutionPayload {
            calls: vec![call],
            auth_witnesses: vec![],
            capsules: vec![],
            extra_hashed_args: vec![],
            fee_payer: Some(self.payment_contract),
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use aztec_core::types::Fr;

    #[tokio::test]
    async fn payload_has_single_call_to_sponsor_unconditionally() {
        let contract = AztecAddress(Fr::from(42u64));
        let method = SponsoredFeePaymentMethod::new(contract);
        let payload = method.get_fee_execution_payload().await.expect("payload");

        assert_eq!(payload.calls.len(), 1);
        let call = &payload.calls[0];
        assert_eq!(call.to, contract);
        assert_eq!(
            call.selector,
            FunctionSelector::from_signature("sponsor_unconditionally()")
        );
        assert!(call.args.is_empty());
        assert_eq!(call.function_type, FunctionType::Private);
        assert!(!call.is_static);
    }

    #[tokio::test]
    async fn fee_payer_is_payment_contract() {
        let contract = AztecAddress(Fr::from(42u64));
        let method = SponsoredFeePaymentMethod::new(contract);

        assert_eq!(method.get_fee_payer().await.expect("fee payer"), contract);

        let payload = method.get_fee_execution_payload().await.expect("payload");
        assert_eq!(payload.fee_payer, Some(contract));
    }

    #[tokio::test]
    async fn get_asset_returns_error() {
        let contract = AztecAddress(Fr::from(42u64));
        let method = SponsoredFeePaymentMethod::new(contract);
        assert!(method.get_asset().await.is_err());
    }
}
