//! Contract test group.
//!
//! Tests primarily exercising the `aztec-contract` crate.
//!
//! Run only this group:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test contract -- --ignored --nocapture
//! ```

#[path = "common/mod.rs"]
pub mod common;

#[path = "contract/e2e_authwit.rs"]
mod e2e_authwit;
#[path = "contract/e2e_contract_updates.rs"]
mod e2e_contract_updates;
#[path = "contract/e2e_deploy_contract_class_registration.rs"]
mod e2e_deploy_contract_class_registration;
#[path = "contract/e2e_deploy_legacy.rs"]
mod e2e_deploy_legacy;
#[path = "contract/e2e_deploy_method.rs"]
mod e2e_deploy_method;
#[path = "contract/e2e_deploy_private_initialization.rs"]
mod e2e_deploy_private_initialization;
#[path = "contract/e2e_escrow_contract.rs"]
mod e2e_escrow_contract;
#[path = "contract/e2e_event_logs.rs"]
mod e2e_event_logs;
#[path = "contract/e2e_event_only.rs"]
mod e2e_event_only;
#[path = "contract/e2e_nested_contract_importer.rs"]
mod e2e_nested_contract_importer;
#[path = "contract/e2e_nested_contract_manual_private_call.rs"]
mod e2e_nested_contract_manual_private_call;
#[path = "contract/e2e_nested_contract_manual_private_enqueue.rs"]
mod e2e_nested_contract_manual_private_enqueue;
#[path = "contract/e2e_nested_contract_manual_public.rs"]
mod e2e_nested_contract_manual_public;
#[path = "contract/e2e_nft.rs"]
mod e2e_nft;
#[path = "contract/e2e_option_params.rs"]
mod e2e_option_params;
#[path = "contract/e2e_ordering.rs"]
mod e2e_ordering;
#[path = "contract/e2e_state_vars.rs"]
mod e2e_state_vars;
#[path = "contract/e2e_static_calls.rs"]
mod e2e_static_calls;
#[path = "contract/e2e_token_access_control.rs"]
mod e2e_token_access_control;
#[path = "contract/e2e_token_burn.rs"]
mod e2e_token_burn;
#[path = "contract/e2e_token_contract_reading_constants.rs"]
mod e2e_token_contract_reading_constants;
#[path = "contract/e2e_token_contract_transfer.rs"]
mod e2e_token_contract_transfer;
#[path = "contract/e2e_token_minting.rs"]
mod e2e_token_minting;
#[path = "contract/e2e_token_transfer_private.rs"]
mod e2e_token_transfer_private;
#[path = "contract/e2e_token_transfer_public.rs"]
mod e2e_token_transfer_public;
#[path = "contract/e2e_token_transfer_recursion.rs"]
mod e2e_token_transfer_recursion;
#[path = "contract/e2e_token_transfer_to_private.rs"]
mod e2e_token_transfer_to_private;
#[path = "contract/e2e_token_transfer_to_public.rs"]
mod e2e_token_transfer_to_public;
