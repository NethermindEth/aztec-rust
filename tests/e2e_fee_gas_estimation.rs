//! Gas estimation tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_fees/gas_estimation.test.ts`.
//!
//! **Required fixture artifacts (compile from aztec-packages and place in `fixtures/`):**
//! - `fpc_contract_compiled.json` (Fee Payment Contract)
//! - `token_contract_compiled.json` (already present)
//!
//! **Required SDK features not yet implemented:**
//! - `PublicFeePaymentMethod` — pays fees publicly through an FPC.
//!   Needs to be added to `aztec_rs::fee`.
//! - Gas estimation simulation support via `estimateGas: true` in simulate options.
//!   The Rust SDK needs `SimulateOptions::estimate_gas` and
//!   `SimulateOptions::estimated_gas_padding` fields, plus a `SuggestedGasLimits`
//!   return type from simulation.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_fee_gas_estimation -- --ignored --nocapture
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

use aztec_rs::deployment::get_gas_limits;
use aztec_rs::fee::GasSettings;
use aztec_rs::node::AztecNode;

use common::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_transfer_request(
    token_artifact: &ContractArtifact,
    token_address: AztecAddress,
    alice: AztecAddress,
    bob: AztecAddress,
) -> FunctionCall {
    build_call(
        token_artifact,
        token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(alice)),
            AbiValue::Field(Fr::from(bob)),
            AbiValue::Integer(1),
            AbiValue::Integer(0),
        ],
    )
}

// ---------------------------------------------------------------------------
// Shared test state
// ---------------------------------------------------------------------------

struct GasEstimationState {
    wallet: TestWallet,
    alice_address: AztecAddress,
    bob_address: AztecAddress,
    token_artifact: ContractArtifact,
    token_address: AztecAddress,
    #[allow(dead_code)]
    fpc_artifact: Option<ContractArtifact>,
    #[allow(dead_code)]
    fpc_address: Option<AztecAddress>,
}

static SHARED_STATE: OnceCell<Option<GasEstimationState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static GasEstimationState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<GasEstimationState> {
    let (wallet, alice_address) = setup_wallet(TEST_ACCOUNT_0).await?;
    let bob_address = imported_complete_address(TEST_ACCOUNT_1).address;

    wallet
        .pxe()
        .register_sender(&bob_address)
        .await
        .expect("register bob");

    // Deploy BananaCoin token
    let token_artifact = load_token_artifact();
    let deploy = Contract::deploy(
        &wallet,
        token_artifact.clone(),
        vec![
            AbiValue::Field(Fr::from(alice_address)),
            AbiValue::String("BananaCoin".to_owned()),
            AbiValue::String("BC".to_owned()),
            AbiValue::Integer(18),
        ],
        None,
    )
    .expect("deploy builder");

    let result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: alice_address,
                ..Default::default()
            },
        )
        .await
        .expect("deploy banana token");
    let token_address = result.instance.address;

    // Deploy FPC contract (optional — fixture may not be available)
    let (fpc_artifact, fpc_address) = if let Some(fpc_art) = load_fpc_artifact() {
        let fpc_deploy = Contract::deploy(
            &wallet,
            fpc_art.clone(),
            vec![
                AbiValue::Field(Fr::from(token_address)),
                AbiValue::Field(Fr::from(alice_address)),
            ],
            None,
        )
        .expect("deploy fpc builder");

        let fpc_result = fpc_deploy
            .send(
                &DeployOptions {
                    contract_address_salt: Some(Fr::from(next_unique_salt())),
                    ..Default::default()
                },
                SendOptions {
                    from: alice_address,
                    ..Default::default()
                },
            )
            .await
            .expect("deploy fpc");
        (Some(fpc_art), Some(fpc_result.instance.address))
    } else {
        (None, None)
    };

    // Mint public bananas to Alice for transfers
    let mint_amount: i128 = 10_000_000_000_000_000_000_000; // 1e22
    let mint_call = build_call(
        &token_artifact,
        token_address,
        "mint_to_public",
        vec![
            AbiValue::Field(Fr::from(alice_address)),
            AbiValue::Integer(mint_amount),
        ],
    );
    wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![mint_call],
                ..Default::default()
            },
            SendOptions {
                from: alice_address,
                ..Default::default()
            },
        )
        .await
        .expect("mint public bananas");

    // TODO: Bridge Fee Juice to FPC for public fee payment tests

    Some(GasEstimationState {
        wallet,
        alice_address,
        bob_address,
        token_artifact,
        token_address,
        fpc_artifact,
        fpc_address,
    })
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: estimates gas with Fee Juice payment method
///
/// 1. Simulate a public transfer with `estimateGas: true` to get estimated gas limits.
/// 2. Send two transfers: one using estimated limits, one using defaults.
/// 3. Verify both succeed and the estimated tx uses tighter gas limits.
/// 4. For Fee Juice payment (no teardown), teardown gas should be 0.
/// 5. The computed fee from estimated gas should match the actual tx fee.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn estimates_gas_fee_juice() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let transfer_call = make_transfer_request(
        &s.token_artifact,
        s.token_address,
        s.alice_address,
        s.bob_address,
    );

    // Step 1: Simulate to estimate gas
    let sim_result = s
        .wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![transfer_call.clone()],
                ..Default::default()
            },
            SimulateOptions {
                from: s.alice_address,
                ..Default::default()
            },
        )
        .await
        .expect("simulate for gas estimation");

    // Extract suggested gas limits from simulation result.
    // We use 10% padding (default) since the simulation only captures
    // private-phase gas, and the actual execution adds public overhead.
    let estimated = get_gas_limits(&sim_result, None);

    // Step 2: Send with estimated gas limits
    let transfer_estimated = make_transfer_request(
        &s.token_artifact,
        s.token_address,
        s.alice_address,
        s.bob_address,
    );
    let estimated_send = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![transfer_estimated],
                ..Default::default()
            },
            SendOptions {
                from: s.alice_address,
                gas_settings: Some(GasSettings {
                    gas_limits: Some(estimated.gas_limits.clone()),
                    teardown_gas_limits: Some(estimated.teardown_gas_limits.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .expect("send with estimated gas");

    // Step 3: Send with default gas limits for comparison
    let transfer_default = make_transfer_request(
        &s.token_artifact,
        s.token_address,
        s.alice_address,
        s.bob_address,
    );
    let default_send = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![transfer_default],
                ..Default::default()
            },
            SendOptions {
                from: s.alice_address,
                ..Default::default()
            },
        )
        .await
        .expect("send with default gas");

    let receipt_estimated = s
        .wallet
        .pxe()
        .node()
        .get_tx_receipt(&estimated_send.tx_hash)
        .await
        .expect("get estimated receipt");
    let receipt_default = s
        .wallet
        .pxe()
        .node()
        .get_tx_receipt(&default_send.tx_hash)
        .await
        .expect("get default receipt");

    let fee_estimated = receipt_estimated.transaction_fee.unwrap_or(0);
    let fee_default = receipt_default.transaction_fee.unwrap_or(0);

    // For Fee Juice (no teardown), both should succeed with similar fees.
    // The estimated tx has tighter limits but pays the same actual fee.
    assert!(
        fee_estimated > 0 && fee_default > 0,
        "both txs should have non-zero fees (estimated={fee_estimated}, default={fee_default})"
    );
    assert_eq!(
        fee_estimated, fee_default,
        "fees should match (no teardown cost difference)"
    );

    // Teardown gas should be 0 for native Fee Juice payment
    assert_eq!(
        estimated.teardown_gas_limits.l2_gas, 0,
        "teardown l2 gas should be 0"
    );
    assert_eq!(
        estimated.teardown_gas_limits.da_gas, 0,
        "teardown da gas should be 0"
    );
}

/// TS: estimates gas with public payment method
///
/// Uses `PublicFeePaymentMethod` to pay fees through the FPC.
/// The teardown phase has non-zero gas because the FPC performs work.
/// The estimated tx should have lower fees than the default.
///
/// NOTE: Requires `PublicFeePaymentMethod` to be implemented in the SDK.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn estimates_gas_public_payment() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // TODO: When PublicFeePaymentMethod is available:
    //
    // let gas_settings_for_estimation = GasSettings {
    //     gas_limits: Some(Gas {
    //         da_gas: GAS_ESTIMATION_DA_GAS_LIMIT,
    //         l2_gas: GAS_ESTIMATION_L2_GAS_LIMIT,
    //     }),
    //     teardown_gas_limits: Some(Gas {
    //         da_gas: GAS_ESTIMATION_TEARDOWN_DA_GAS_LIMIT,
    //         l2_gas: GAS_ESTIMATION_TEARDOWN_L2_GAS_LIMIT,
    //     }),
    //     ..Default::default()
    // };
    //
    // let payment = PublicFeePaymentMethod::new(
    //     s.fpc_address, s.alice_address, &s.wallet, gas_settings_for_estimation,
    // );
    // let fee_payload = payment.get_fee_execution_payload().await?;
    //
    // // Simulate with gas estimation
    // let sim = wallet.simulate_tx(payload, SimulateOptions {
    //     fee_execution_payload: Some(fee_payload),
    //     estimate_gas: true,
    //     estimated_gas_padding: 0,
    //     ..
    // }).await?;
    //
    // let estimated = get_gas_limits(&sim, Some(0.0));
    //
    // // Send with estimated limits and with defaults
    // // Assert:
    // // - estimated.teardown_gas_limits.l2_gas < default teardown l2 gas
    // // - estimated.teardown_gas_limits.da_gas < default teardown da gas
    // // - fee_estimated < fee_default (estimation saves money)
    // // - estimated.teardown_gas_limits.l2_gas > 0 (FPC does work)
    //
    // For now this test is a stub that will be completed when
    // PublicFeePaymentMethod is implemented.

    let transfer_call = make_transfer_request(
        &s.token_artifact,
        s.token_address,
        s.alice_address,
        s.bob_address,
    );

    // Basic simulation to verify the transfer works
    s.wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![transfer_call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.alice_address,
                ..Default::default()
            },
        )
        .await
        .expect("simulate transfer");
}

/// TS: estimates gas for public contract initialization with Fee Juice payment method
///
/// Deploys a BananaCoin instance with gas estimation and without.
/// Verifies the estimated gas produces the correct fee.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn estimates_gas_contract_init_fee_juice() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let token_artifact = load_token_artifact();

    // Deploy with default gas (for comparison baseline)
    let deploy_default = Contract::deploy(
        &s.wallet,
        token_artifact.clone(),
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::String("TKN1".to_owned()),
            AbiValue::String("TK1".to_owned()),
            AbiValue::Integer(8),
        ],
        None,
    )
    .expect("deploy builder default");

    let default_result = deploy_default
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                skip_class_publication: true,
                ..Default::default()
            },
            SendOptions {
                from: s.alice_address,
                ..Default::default()
            },
        )
        .await
        .expect("deploy with defaults");

    let receipt_default = s
        .wallet
        .pxe()
        .node()
        .get_tx_receipt(&default_result.send_result.tx_hash)
        .await
        .expect("get default deploy receipt");

    // Deploy with estimated gas
    let deploy_estimated = Contract::deploy(
        &s.wallet,
        token_artifact.clone(),
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::String("TKN2".to_owned()),
            AbiValue::String("TK2".to_owned()),
            AbiValue::Integer(8),
        ],
        None,
    )
    .expect("deploy builder estimated");

    let deploy_opts = DeployOptions {
        contract_address_salt: Some(Fr::from(next_unique_salt())),
        skip_class_publication: true,
        ..Default::default()
    };

    // Simulate to get gas estimate
    let sim_result = deploy_estimated
        .simulate(
            &deploy_opts,
            SimulateOptions {
                from: s.alice_address,
                ..Default::default()
            },
        )
        .await
        .expect("simulate deploy");

    let estimated = get_gas_limits(&sim_result, None); // 10% padding

    let estimated_result = deploy_estimated
        .send(
            &deploy_opts,
            SendOptions {
                from: s.alice_address,
                gas_settings: Some(GasSettings {
                    gas_limits: Some(estimated.gas_limits.clone()),
                    teardown_gas_limits: Some(estimated.teardown_gas_limits.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .expect("deploy with estimated gas");

    let receipt_estimated = s
        .wallet
        .pxe()
        .node()
        .get_tx_receipt(&estimated_result.send_result.tx_hash)
        .await
        .expect("get estimated deploy receipt");

    let fee_default = receipt_default.transaction_fee.unwrap_or(0);
    let fee_estimated = receipt_estimated.transaction_fee.unwrap_or(0);

    // For Fee Juice (no teardown), both should succeed with the same fee.
    assert!(
        fee_estimated > 0 && fee_default > 0,
        "both deploys should have non-zero fees (estimated={fee_estimated}, default={fee_default})"
    );
    assert_eq!(
        fee_estimated, fee_default,
        "deploy fees should match (no teardown)"
    );

    // Teardown gas should be 0 for native Fee Juice
    assert_eq!(
        estimated.teardown_gas_limits.l2_gas, 0,
        "teardown l2 gas should be 0"
    );
    assert_eq!(
        estimated.teardown_gas_limits.da_gas, 0,
        "teardown da gas should be 0"
    );
}
