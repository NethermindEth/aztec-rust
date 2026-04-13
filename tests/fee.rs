//! Fee test group.
//!
//! Tests primarily exercising the `aztec-fee` crate.
//!
//! Run only this group:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test fee -- --ignored --nocapture
//! ```

#[path = "common/mod.rs"]
pub mod common;

#[path = "fee/e2e_fee_account_init.rs"]
mod e2e_fee_account_init;
#[path = "fee/e2e_fee_failures.rs"]
mod e2e_fee_failures;
#[path = "fee/e2e_fee_gas_estimation.rs"]
mod e2e_fee_gas_estimation;
#[path = "fee/e2e_fee_juice_payments.rs"]
mod e2e_fee_juice_payments;
#[path = "fee/e2e_fee_private_payments.rs"]
mod e2e_fee_private_payments;
#[path = "fee/e2e_fee_public_payments.rs"]
mod e2e_fee_public_payments;
#[path = "fee/e2e_fee_settings.rs"]
mod e2e_fee_settings;
#[path = "fee/e2e_fee_sponsored_payments.rs"]
mod e2e_fee_sponsored_payments;
