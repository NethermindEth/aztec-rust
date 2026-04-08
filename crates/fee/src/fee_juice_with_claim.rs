use async_trait::async_trait;
use aztec_core::abi::{AbiValue, FunctionSelector, FunctionType};
use aztec_core::constants::protocol_contract_address;
use aztec_core::tx::{ExecutionPayload, FunctionCall};
use aztec_core::types::{AztecAddress, Fr};
use aztec_core::Error;

use crate::fee_payment_method::FeePaymentMethod;
use crate::types::L2AmountClaim;

/// Pays transaction fees by claiming Fee Juice from an L1-to-L2 bridge deposit.
///
/// This constructs a call to the FeeJuice protocol contract's
/// `claim_and_end_setup` function, which:
/// 1. Consumes the L1-to-L2 message (proving the L1 deposit)
/// 2. Credits the sender's Fee Juice balance
/// 3. Ends the transaction setup phase (making the balance available for fees)
///
/// The claim is placed in the non-revertible phase so the sequencer is
/// guaranteed to collect fees even if the revertible portion fails.
pub struct FeeJuicePaymentMethodWithClaim {
    /// Address of the account claiming and paying fees.
    sender: AztecAddress,
    /// Claim data from the L1 bridge deposit.
    claim: L2AmountClaim,
}

impl FeeJuicePaymentMethodWithClaim {
    /// Create a new fee payment method that claims bridged Fee Juice.
    ///
    /// `sender` is the account that will pay fees after claiming.
    /// `claim` contains the L1 bridge deposit data needed for the claim.
    pub fn new(sender: AztecAddress, claim: L2AmountClaim) -> Self {
        Self { sender, claim }
    }
}

#[async_trait]
impl FeePaymentMethod for FeeJuicePaymentMethodWithClaim {
    async fn get_asset(&self) -> Result<AztecAddress, Error> {
        Ok(protocol_contract_address::fee_juice())
    }

    async fn get_fee_payer(&self) -> Result<AztecAddress, Error> {
        Ok(self.sender)
    }

    async fn get_fee_execution_payload(&self) -> Result<ExecutionPayload, Error> {
        let call = FunctionCall {
            to: protocol_contract_address::fee_juice(),
            selector: FunctionSelector::from_signature(
                "claim_and_end_setup((Field),u128,Field,Field)",
            ),
            args: vec![
                AbiValue::Field(self.sender.0),
                AbiValue::Integer(self.claim.claim_amount as i128),
                AbiValue::Field(self.claim.claim_secret),
                AbiValue::Field(Fr::from(self.claim.message_leaf_index)),
            ],
            function_type: FunctionType::Private,
            is_static: false,
            hide_msg_sender: false,
        };

        Ok(ExecutionPayload {
            calls: vec![call],
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

    fn test_claim() -> L2AmountClaim {
        L2AmountClaim {
            claim_amount: 1000,
            claim_secret: Fr::from(99u64),
            message_leaf_index: 7,
        }
    }

    #[tokio::test]
    async fn payload_targets_fee_juice_contract() {
        let sender = AztecAddress(Fr::from(1u64));
        let method = FeeJuicePaymentMethodWithClaim::new(sender, test_claim());
        let payload = method.get_fee_execution_payload().await.expect("payload");

        assert_eq!(payload.calls.len(), 1);
        let call = &payload.calls[0];
        assert_eq!(call.to, protocol_contract_address::fee_juice());
        assert_eq!(
            call.selector,
            FunctionSelector::from_signature("claim_and_end_setup((Field),u128,Field,Field)")
        );
        assert_eq!(call.function_type, FunctionType::Private);
        assert!(!call.is_static);
    }

    #[tokio::test]
    async fn arguments_are_correctly_ordered() {
        let sender = AztecAddress(Fr::from(1u64));
        let claim = test_claim();
        let method = FeeJuicePaymentMethodWithClaim::new(sender, claim.clone());
        let payload = method.get_fee_execution_payload().await.expect("payload");

        let args = &payload.calls[0].args;
        assert_eq!(args.len(), 4);

        // arg 0: sender as struct { inner: Field }
        assert_eq!(args[0], AbiValue::Field(sender.0));
        // arg 1: claim_amount
        assert_eq!(args[1], AbiValue::Integer(claim.claim_amount as i128));
        // arg 2: claim_secret
        assert_eq!(args[2], AbiValue::Field(claim.claim_secret));
        // arg 3: message_leaf_index as Field
        assert_eq!(args[3], AbiValue::Field(Fr::from(claim.message_leaf_index)));
    }

    #[tokio::test]
    async fn fee_payer_is_sender() {
        let sender = AztecAddress(Fr::from(1u64));
        let method = FeeJuicePaymentMethodWithClaim::new(sender, test_claim());

        assert_eq!(method.get_fee_payer().await.expect("fee payer"), sender);

        let payload = method.get_fee_execution_payload().await.expect("payload");
        assert_eq!(payload.fee_payer, Some(sender));
    }

    #[tokio::test]
    async fn asset_is_fee_juice() {
        let sender = AztecAddress(Fr::from(1u64));
        let method = FeeJuicePaymentMethodWithClaim::new(sender, test_claim());
        assert_eq!(
            method.get_asset().await.expect("asset"),
            protocol_contract_address::fee_juice()
        );
    }
}
