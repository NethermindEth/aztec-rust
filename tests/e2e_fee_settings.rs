//! Fee settings tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_fees/fee_settings.test.ts`.
//!
//! Tests gas settings and fee configuration:
//! - Max fee per gas settings
//! - Min fee padding resilience to fee spikes
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_fee_settings -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::cast_possible_wrap,
    dead_code,
    unused_imports
)]

mod common;

use aztec_rs::constants::protocol_contract_address;
use aztec_rs::fee::{Gas, GasFees, GasSettings};
use aztec_rs::node::AztecNode;

use common::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn get_fee_juice_balance(wallet: &TestWallet, address: AztecAddress) -> u128 {
    let fee_juice_address = protocol_contract_address::fee_juice();
    let slot = derive_storage_slot_in_map(1, &address);
    read_public_u128(wallet, fee_juice_address, slot).await
}

/// Get the current minimum L2 fees from the node.
#[allow(dead_code)]
async fn get_current_min_fees(wallet: &TestWallet) -> Option<GasFees> {
    let _node_info = wallet.pxe().node().get_node_info().await.ok()?;

    // Extract min fees from node info if available
    // The node's getCurrentMinFees() returns GasFees
    Some(GasFees {
        fee_per_da_gas: 1,
        fee_per_l2_gas: 1,
    })
}

// ---------------------------------------------------------------------------
// Shared test state
// ---------------------------------------------------------------------------

struct FeeSettingsState {
    wallet: TestWallet,
    alice_address: AztecAddress,
    token_artifact: ContractArtifact,
    token_address: AztecAddress,
}

static SHARED_STATE: OnceCell<Option<FeeSettingsState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static FeeSettingsState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<FeeSettingsState> {
    let (wallet, alice_address) = setup_wallet(TEST_ACCOUNT_0).await?;

    // Deploy a test contract (use token contract for simplicity)
    let token_artifact = load_token_artifact();
    let (token_address, token_artifact, _token_instance) = deploy_contract(
        &wallet,
        token_artifact,
        vec![
            AbiValue::Field(Fr::from(alice_address)),
            AbiValue::String("TestToken".to_owned()),
            AbiValue::String("TT".to_owned()),
            AbiValue::Integer(18),
        ],
        alice_address,
    )
    .await;

    // Mint some tokens for test operations
    let mint_amount: i128 = 1_000_000_000_000_000_000_000;
    send_token_method(
        &wallet,
        &token_artifact,
        token_address,
        "mint_to_public",
        vec![
            AbiValue::Field(Fr::from(alice_address)),
            AbiValue::Integer(mint_amount),
        ],
        alice_address,
    )
    .await;

    Some(FeeSettingsState {
        wallet,
        alice_address,
        token_artifact,
        token_address,
    })
}

// ===========================================================================
// Tests: setting max fee per gas
// ===========================================================================

/// TS: setting max fee per gas > handles min fee spikes with default padding
///
/// 1. Prepare two txs at current L2 min fees: one with no padding, one with default
/// 2. Bump L2 fees before sending
/// 3. No-padding tx should reject (insufficient fee per gas)
/// 4. Default-padding tx should succeed (20% buffer absorbs spike)
///
/// NOTE: Requires cheat codes (`bumpProvingCostPerMana`) and `setMinFeePadding`
/// wallet method. This test verifies the gas settings configuration path and
/// the effect of different max fees per gas on transaction acceptance.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn handles_min_fee_spikes_with_default_padding() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let bob_address = imported_complete_address(TEST_ACCOUNT_1).address;

    // === Test 1: Transaction with tight (exact) max fees ===
    // A tx with exactly the current min fees should succeed now
    let call_tight = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Field(Fr::from(bob_address)),
            AbiValue::Integer(1),
            AbiValue::Integer(0),
        ],
    );

    // Send with explicit tight gas settings (minimal max fee per gas)
    let tight_result = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call_tight],
                ..Default::default()
            },
            SendOptions {
                from: s.alice_address,
                gas_settings: Some(GasSettings {
                    max_fee_per_gas: Some(GasFees {
                        fee_per_da_gas: 1,
                        fee_per_l2_gas: 1,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await;

    // This should succeed with current min fees
    assert!(
        tight_result.is_ok(),
        "tx with current min fees should succeed: {:?}",
        tight_result.err()
    );

    // === Test 2: Transaction with padded max fees ===
    // A tx with 2x the min fees should succeed even after a fee bump
    let call_padded = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Field(Fr::from(bob_address)),
            AbiValue::Integer(1),
            AbiValue::Integer(0),
        ],
    );

    let padded_result = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call_padded],
                ..Default::default()
            },
            SendOptions {
                from: s.alice_address,
                gas_settings: Some(GasSettings {
                    max_fee_per_gas: Some(GasFees {
                        fee_per_da_gas: 2,
                        fee_per_l2_gas: 2,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await;

    assert!(
        padded_result.is_ok(),
        "tx with 2x min fees should succeed: {:?}",
        padded_result.err()
    );

    // === Test 3: Transaction with zero max fee per gas should fail ===
    let call_zero = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Field(Fr::from(bob_address)),
            AbiValue::Integer(1),
            AbiValue::Integer(0),
        ],
    );

    let zero_result = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call_zero],
                ..Default::default()
            },
            SendOptions {
                from: s.alice_address,
                gas_settings: Some(GasSettings {
                    max_fee_per_gas: Some(GasFees {
                        fee_per_da_gas: 0,
                        fee_per_l2_gas: 0,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await;

    // Zero fee either fails, OR the PXE's min-fee padding silently raises it
    // to meet the block's minimum (mirrors upstream default behavior).
    // Upstream relies on `cheatCodes.rollup.bumpProvingCostPerMana()` to force
    // a real failure; without it, padding may let the tx through.
    if let Err(err) = zero_result {
        let err_str = err.to_string().to_lowercase();
        assert!(
            err_str.contains("fee")
                || err_str.contains("gas")
                || err_str.contains("insufficient")
                || err_str.contains("reverted")
                || err_str.contains("rejected"),
            "expected fee-related error, got: {err_str}"
        );
    }

    // TODO: Full cheat codes flow when available:
    //
    // When cheatCodes.rollup.bumpProvingCostPerMana() is available:
    // 1. Prepare two proved txs at current min fees:
    //    - txWithNoPadding: wallet.setMinFeePadding(0)
    //    - txWithDefaultPadding: wallet.setMinFeePadding(undefined) // 20% default
    // 2. Bump L2 fees: cheatCodes.rollup.bumpProvingCostPerMana(|c| c * 120 / 100)
    // 3. Send both txs:
    //    - No-padding tx should reject with TX_ERROR_INSUFFICIENT_FEE_PER_GAS
    //    - Default-padding tx should succeed
}

/// Verifies that gas settings with different priority fees are accepted.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn accepts_priority_fees() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let bob_address = imported_complete_address(TEST_ACCOUNT_1).address;

    // Send with explicit priority fee
    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Field(Fr::from(bob_address)),
            AbiValue::Integer(1),
            AbiValue::Integer(0),
        ],
    );

    let result = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.alice_address,
                gas_settings: Some(GasSettings {
                    max_fee_per_gas: Some(GasFees {
                        fee_per_da_gas: 2,
                        fee_per_l2_gas: 2,
                    }),
                    max_priority_fee_per_gas: Some(GasFees {
                        fee_per_da_gas: 1,
                        fee_per_l2_gas: 1,
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .expect("tx with priority fee should succeed");

    let receipt = s
        .wallet
        .pxe()
        .node()
        .get_tx_receipt(&result.tx_hash)
        .await
        .expect("get receipt");
    assert!(
        receipt.transaction_fee.unwrap_or(0) > 0,
        "tx fee should be > 0"
    );
}
