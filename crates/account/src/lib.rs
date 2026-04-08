//! Account abstractions and deployment helpers.

pub mod abi {
    pub use aztec_core::abi::*;
}

pub mod error {
    pub use aztec_core::error::*;
}

pub mod fee {
    pub use aztec_core::fee::*;
}

pub mod tx {
    pub use aztec_core::tx::*;
}

pub mod types {
    pub use aztec_core::types::*;
}

pub mod wallet {
    pub use aztec_wallet::*;
}

pub mod account;
pub mod authorization;
pub mod entrypoint;
pub mod meta_payment;
pub mod schnorr;
pub mod signerless;
pub mod single_account_provider;

pub use account::*;
pub use authorization::CallAuthorizationRequest;
pub use entrypoint::{
    AccountFeePaymentMethodOptions, DefaultAccountEntrypoint, DefaultAccountEntrypointOptions,
    DefaultMultiCallEntrypoint, EncodedAppEntrypointCalls,
};
pub use error::Error;
pub use meta_payment::AccountEntrypointMetaPaymentMethod;
pub use schnorr::SchnorrAccountContract;
pub use signerless::SignerlessAccount;
pub use single_account_provider::SingleAccountProvider;
