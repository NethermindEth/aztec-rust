use aztec_core::types::Fr;
use serde::{Deserialize, Serialize};

/// Data required to claim bridged Fee Juice on L2.
///
/// This is produced by the L1 Fee Juice portal when a deposit is made,
/// and consumed by [`FeeJuicePaymentMethodWithClaim`](crate::FeeJuicePaymentMethodWithClaim)
/// to claim the deposit in the same transaction as fee payment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2AmountClaim {
    /// Amount of Fee Juice being claimed (u128 in Noir).
    pub claim_amount: u128,
    /// Secret used to consume the L1-to-L2 message.
    pub claim_secret: Fr,
    /// Index of the message leaf in the L1-to-L2 message tree.
    pub message_leaf_index: u64,
}
