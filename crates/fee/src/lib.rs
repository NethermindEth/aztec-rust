//! Fee payment strategies for aztec-rs.
//!
//! This crate provides implementations of the [`FeePaymentMethod`] trait,
//! which determines how transaction fees are paid on the Aztec network.
//!
//! # Available strategies
//!
//! - [`NativeFeePaymentMethod`] — pay with existing Fee Juice balance (default)
//! - [`SponsoredFeePaymentMethod`] — a sponsor contract pays unconditionally
//! - [`FeeJuicePaymentMethodWithClaim`] — claim Fee Juice from L1 bridge and pay

mod fee_juice_with_claim;
mod fee_payment_method;
mod native;
mod sponsored;
mod types;

pub use fee_juice_with_claim::FeeJuicePaymentMethodWithClaim;
pub use fee_payment_method::FeePaymentMethod;
pub use native::NativeFeePaymentMethod;
pub use sponsored::SponsoredFeePaymentMethod;
pub use types::L2AmountClaim;
