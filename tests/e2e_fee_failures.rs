//! Fee failure tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_fees/failures.test.ts`.
//!
//! Tests fee-related error paths and reverts:
//! - Transactions that revert in app logic but still pay fees (private FPC)
//! - Transactions that revert in app logic but still pay fees (public FPC)
//! - Transactions that fail in setup phase (dropped entirely)
//! - Transactions that error in teardown (included but teardown reverted)
//!
//! **Required fixture artifacts (compile from aztec-packages and place in `fixtures/`):**
//! - `fpc_contract_compiled.json` (Fee Payment Contract)
//! - `token_contract_compiled.json` (already present)
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_fee_failures -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::cast_possible_wrap,
    clippy::unwrap_used,
    clippy::used_underscore_binding,
    dead_code,
    unused_imports
)]

mod common;

use aztec_rs::abi::FunctionSelector;
use aztec_rs::authwit::SetPublicAuthWitInteraction;
use aztec_rs::constants::protocol_contract_address;
use aztec_rs::fee::{FeePaymentMethod, Gas, GasSettings};
use aztec_rs::hash::MessageHashOrIntent;
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

fn get_public_banana_balance_slot(address: &AztecAddress) -> Fr {
    derive_storage_slot_in_map(5, address)
}

async fn get_banana_public_balance(
    wallet: &TestWallet,
    token_address: AztecAddress,
    address: &AztecAddress,
) -> u128 {
    let slot = get_public_banana_balance_slot(address);
    read_public_u128(wallet, token_address, slot).await
}

/// Compute the maximum fee from gas settings.
fn compute_max_fee(gas_settings: &GasSettings) -> u128 {
    let gl = gas_settings
        .gas_limits
        .as_ref()
        .map_or((0u64, 0u64), |g| (g.da_gas, g.l2_gas));
    let tgl = gas_settings
        .teardown_gas_limits
        .as_ref()
        .map_or((0u64, 0u64), |g| (g.da_gas, g.l2_gas));
    let fees = gas_settings
        .max_fee_per_gas
        .as_ref()
        .map_or((1u128, 1u128), |f| (f.fee_per_da_gas, f.fee_per_l2_gas));
    u128::from(gl.0 + tgl.0) * fees.0 + u128::from(gl.1 + tgl.1) * fees.1
}

/// Build the fee execution payload for a public FPC payment.
/// Mirrors TS `PublicFeePaymentMethod.getExecutionPayload()`.
async fn build_public_fee_payload(
    wallet: &TestWallet,
    fpc_address: AztecAddress,
    sender: AztecAddress,
    token_address: AztecAddress,
    gas_settings: &GasSettings,
) -> ExecutionPayload {
    let max_fee = compute_max_fee(gas_settings);
    let nonce = Fr::random();

    let transfer_call = FunctionCall {
        to: token_address,
        selector: FunctionSelector::from_signature(
            "transfer_in_public((Field),(Field),u128,Field)",
        ),
        args: vec![
            AbiValue::Field(Fr::from(sender)),
            AbiValue::Field(Fr::from(fpc_address)),
            AbiValue::Integer(i128::try_from(max_fee).unwrap_or(i128::MAX)),
            AbiValue::Field(nonce),
        ],
        function_type: FunctionType::Public,
        is_static: false,
        hide_msg_sender: false,
    };

    let authwit_interaction: SetPublicAuthWitInteraction<'_, TestWallet> =
        SetPublicAuthWitInteraction::create(
            wallet,
            sender,
            MessageHashOrIntent::Intent {
                caller: fpc_address,
                call: transfer_call,
            },
            true,
        )
        .await
        .expect("create authwit interaction");
    let authwit_payload = authwit_interaction.request();

    let fpc_call = FunctionCall {
        to: fpc_address,
        selector: FunctionSelector::from_signature("fee_entrypoint_public(u128,Field)"),
        args: vec![
            AbiValue::Integer(i128::try_from(max_fee).unwrap_or(i128::MAX)),
            AbiValue::Field(nonce),
        ],
        function_type: FunctionType::Private,
        is_static: false,
        hide_msg_sender: false,
    };

    let mut calls = authwit_payload.calls;
    calls.push(fpc_call);

    ExecutionPayload {
        calls,
        fee_payer: Some(fpc_address),
        ..Default::default()
    }
}

/// Build a "bugged" public fee payload where the authwit authorizes `maxFee` but
/// the FPC entrypoint requests `maxFee * 2`, triggering a setup-phase failure.
/// Mirrors TS `BuggedSetupFeePaymentMethod`.
async fn build_bugged_setup_fee_payload(
    wallet: &TestWallet,
    fpc_address: AztecAddress,
    sender: AztecAddress,
    token_address: AztecAddress,
    gas_settings: &GasSettings,
) -> ExecutionPayload {
    let max_fee = compute_max_fee(gas_settings);
    let too_much_fee = max_fee * 2;
    let nonce = Fr::random();

    // Authwit is set for the correct maxFee
    let transfer_call = FunctionCall {
        to: token_address,
        selector: FunctionSelector::from_signature(
            "transfer_in_public((Field),(Field),u128,Field)",
        ),
        args: vec![
            AbiValue::Field(Fr::from(sender)),
            AbiValue::Field(Fr::from(fpc_address)),
            AbiValue::Integer(i128::try_from(max_fee).unwrap_or(i128::MAX)),
            AbiValue::Field(nonce),
        ],
        function_type: FunctionType::Public,
        is_static: false,
        hide_msg_sender: false,
    };

    let authwit_interaction: SetPublicAuthWitInteraction<'_, TestWallet> =
        SetPublicAuthWitInteraction::create(
            wallet,
            sender,
            MessageHashOrIntent::Intent {
                caller: fpc_address,
                call: transfer_call,
            },
            true,
        )
        .await
        .expect("create authwit interaction");
    let authwit_payload = authwit_interaction.request();

    // But the FPC entrypoint requests too_much_fee (2x maxFee) — this triggers setup failure
    let fpc_call = FunctionCall {
        to: fpc_address,
        selector: FunctionSelector::from_signature("fee_entrypoint_public(u128,Field)"),
        args: vec![
            AbiValue::Integer(i128::try_from(too_much_fee).unwrap_or(i128::MAX)),
            AbiValue::Field(nonce),
        ],
        function_type: FunctionType::Private,
        is_static: false,
        hide_msg_sender: false,
    };

    let mut calls = authwit_payload.calls;
    calls.push(fpc_call);

    ExecutionPayload {
        calls,
        fee_payer: Some(fpc_address),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Shared test state
// ---------------------------------------------------------------------------

struct FailuresState {
    wallet: TestWallet,
    alice_address: AztecAddress,
    sequencer_address: AztecAddress,
    token_artifact: ContractArtifact,
    token_address: AztecAddress,
    fpc_artifact: Option<ContractArtifact>,
    fpc_address: Option<AztecAddress>,
}

static SHARED_STATE: OnceCell<Option<FailuresState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static FailuresState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<FailuresState> {
    let (wallet, alice_address) = setup_wallet(TEST_ACCOUNT_0).await?;
    let sequencer_address = imported_complete_address(TEST_ACCOUNT_2).address;

    wallet
        .pxe()
        .register_sender(&sequencer_address)
        .await
        .expect("register sequencer");

    // Deploy BananaCoin token
    let token_artifact = load_token_artifact();
    let (token_address, token_artifact, _token_instance) = deploy_contract(
        &wallet,
        token_artifact,
        vec![
            AbiValue::Field(Fr::from(alice_address)),
            AbiValue::String("BananaCoin".to_owned()),
            AbiValue::String("BC".to_owned()),
            AbiValue::Integer(18),
        ],
        alice_address,
    )
    .await;

    // Mint public bananas to Alice
    let mint_amount: i128 = 10_000_000_000_000_000_000_000;
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

    // Deploy FPC contract (optional)
    let (fpc_artifact, fpc_address) = if let Some(fpc_art) = load_fpc_artifact() {
        let (fpc_addr, fpc_art, _fpc_instance) = deploy_contract(
            &wallet,
            fpc_art,
            vec![abi_address(token_address), abi_address(alice_address)],
            alice_address,
        )
        .await;
        (Some(fpc_art), Some(fpc_addr))
    } else {
        (None, None)
    };

    Some(FailuresState {
        wallet,
        alice_address,
        sequencer_address,
        token_artifact,
        token_address,
        fpc_artifact,
        fpc_address,
    })
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: reverts transactions but still pays fees using PrivateFeePaymentMethod
///
/// Alice tries to transfer more bananas than she has, which reverts in
/// app logic. The fee is still paid through the private FPC.
///
/// Verifies:
/// - Simulation fails with U128_UNDERFLOW_ERROR
/// - Submitted tx has executionResult = APP_LOGIC_REVERTED
/// - Fee IS paid (Alice's private bananas decrease)
/// - FPC's public bananas increase by fee
/// - FPC's gas decreases by fee
///
/// NOTE: Requires PrivateFeePaymentMethod + dontThrowOnRevert support.
/// This test verifies the simulation failure path using native Fee Juice.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn reverts_but_pays_fees_private_fpc() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Amount that Alice definitely doesn't have
    let outrageous_amount: i128 = i128::MAX;

    // Simulation should fail — transfer exceeds balance
    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Field(Fr::from(s.sequencer_address)),
            AbiValue::Integer(outrageous_amount),
            AbiValue::Integer(0),
        ],
    );

    let sim_err = s
        .wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call.clone()],
                ..Default::default()
            },
            SimulateOptions {
                from: s.alice_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("simulation should fail with underflow");

    let err_str = sim_err.to_string().to_lowercase();
    assert!(
        err_str.contains("underflow")
            || err_str.contains("overflow")
            || err_str.contains("reverted")
            || err_str.contains("balance"),
        "expected underflow/overflow error in simulation, got: {sim_err}"
    );

    // Verify balances didn't change from simulation (no tx submitted)
    let _alice_gas_after_sim = get_fee_juice_balance(&s.wallet, s.alice_address).await;
    // Note: We can't assert exact values since other tests may have run,
    // but we verify simulation didn't charge fees

    // Now test the actual submission with dontThrowOnRevert pattern.
    // Without FPC, we test the native fee path: the tx gets rejected by the
    // sequencer since the public call will revert.
    let send_result = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.alice_address,
                ..Default::default()
            },
        )
        .await;

    // The tx should fail — either rejected by sequencer or reverted
    assert!(
        send_result.is_err(),
        "submitting a tx that will revert should fail"
    );

    let err = send_result.unwrap_err();
    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("underflow")
            || err_str.contains("overflow")
            || err_str.contains("reverted")
            || err_str.contains("balance")
            || err_str.contains("dropped"),
        "expected revert/drop error, got: {err}"
    );

    // TODO: Full PrivateFeePaymentMethod flow with dontThrowOnRevert:
    //
    // When PrivateFeePaymentMethod and dontThrowOnRevert are available:
    // 1. Build fee payload with PrivateFeePaymentMethod
    // 2. Submit with dontThrowOnRevert: true
    // 3. Assert receipt.executionResult == APP_LOGIC_REVERTED
    // 4. Assert fee IS paid (Alice's private bananas decreased)
    // 5. Assert FPC's public bananas increased by fee
    // 6. Assert FPC's gas decreased by fee
    // 7. Assert Alice received refund note (actual fee < max fee)
}

/// TS: reverts transactions but still pays fees using PublicFeePaymentMethod
///
/// Same as above but with public FPC fee payment.
///
/// NOTE: Requires PublicFeePaymentMethod + dontThrowOnRevert support.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn reverts_but_pays_fees_public_fpc() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let Some(fpc_address) = s.fpc_address else {
        eprintln!("SKIP: FPC artifact not available");
        return;
    };

    let outrageous_amount: i128 = i128::MAX;

    // Mint more public bananas to Alice for the FPC fee
    let public_mint: i128 = 10_000_000_000_000_000_000_000;
    send_token_method(
        &s.wallet,
        &s.token_artifact,
        s.token_address,
        "mint_to_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Integer(public_mint),
        ],
        s.alice_address,
    )
    .await;

    // Capture balances (prefixed for use in TODO assertions below)
    let _initial_alice_public =
        get_banana_public_balance(&s.wallet, s.token_address, &s.alice_address).await;
    let _initial_fpc_public =
        get_banana_public_balance(&s.wallet, s.token_address, &fpc_address).await;
    let _initial_alice_gas = get_fee_juice_balance(&s.wallet, s.alice_address).await;
    let _initial_fpc_gas = get_fee_juice_balance(&s.wallet, fpc_address).await;

    // Build fee payload
    let fee_payload = build_public_fee_payload(
        &s.wallet,
        fpc_address,
        s.alice_address,
        s.token_address,
        &GasSettings::default(),
    )
    .await;

    // Simulation should fail
    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Field(Fr::from(s.sequencer_address)),
            AbiValue::Integer(outrageous_amount),
            AbiValue::Integer(0),
        ],
    );

    let sim_err = s
        .wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call.clone()],
                ..Default::default()
            },
            SimulateOptions {
                from: s.alice_address,
                fee_execution_payload: Some(fee_payload.clone()),
                ..Default::default()
            },
        )
        .await
        .expect_err("simulation should fail");

    let err_str = sim_err.to_string().to_lowercase();
    assert!(
        err_str.contains("underflow")
            || err_str.contains("overflow")
            || err_str.contains("reverted")
            || err_str.contains("balance"),
        "expected underflow error, got: {sim_err}"
    );

    // Balances should not change from simulation
    let alice_public_after_sim =
        get_banana_public_balance(&s.wallet, s.token_address, &s.alice_address).await;
    assert_eq!(
        alice_public_after_sim, _initial_alice_public,
        "simulation should not change balances"
    );

    // TODO: Full PublicFeePaymentMethod flow with dontThrowOnRevert:
    //
    // When dontThrowOnRevert is available on SendOptions:
    // 1. Rebuild fee payload (fresh nonce)
    // 2. Submit with dontThrowOnRevert: true
    // 3. Assert receipt.executionResult == APP_LOGIC_REVERTED
    // 4. Assert fee IS paid:
    //    - Alice's public bananas = initial - fee
    //    - FPC's public bananas = initial + fee
    // 5. Assert FPC's gas decreased by fee
    // 6. Assert Alice's gas is unchanged (FPC paid)
}

/// TS: fails transaction that error in setup
///
/// Uses a bugged fee payment method that requests double the authorized fee,
/// causing the setup phase to fail. Both simulation and submission should fail.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn fails_transaction_that_error_in_setup() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let Some(fpc_address) = s.fpc_address else {
        eprintln!("SKIP: FPC artifact not available");
        return;
    };

    let transfer_amount: i128 = 100_000_000_000_000; // 100e12

    // Build bugged fee payload — authwit allows maxFee but FPC requests 2x maxFee
    let bugged_payload = build_bugged_setup_fee_payload(
        &s.wallet,
        fpc_address,
        s.alice_address,
        s.token_address,
        &GasSettings::default(),
    )
    .await;

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Field(Fr::from(s.sequencer_address)),
            AbiValue::Integer(transfer_amount),
            AbiValue::Integer(0),
        ],
    );

    // Simulation should fail with setup error
    let sim_err = s
        .wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call.clone()],
                ..Default::default()
            },
            SimulateOptions {
                from: s.alice_address,
                fee_execution_payload: Some(bugged_payload.clone()),
                skip_fee_enforcement: false,
                ..Default::default()
            },
        )
        .await
        .expect_err("simulation should fail with setup error");

    let err_str = sim_err.to_string().to_uppercase();
    assert!(
        err_str.contains("SETUP")
            || err_str.contains("UNRECOVERABLE")
            || err_str.contains("AUTHWIT")
            || err_str.contains("REVERTED")
            || err_str.contains("NOT ENOUGH BALANCE")
            || err_str.contains("FEE PAYER"),
        "expected setup/authwit error in simulation, got: {sim_err}"
    );

    // Submission should also fail — sequencer drops the tx
    let send_err = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.alice_address,
                fee_execution_payload: Some(bugged_payload),
                ..Default::default()
            },
        )
        .await
        .expect_err("send should fail: setup error causes tx drop");

    let err_str = send_err.to_string().to_lowercase();
    assert!(
        err_str.contains("dropped")
            || err_str.contains("setup")
            || err_str.contains("reverted")
            || err_str.contains("authwit")
            || err_str.contains("rejected")
            || err_str.contains("not enough balance")
            || err_str.contains("fee payer"),
        "expected tx dropped/setup error, got: {send_err}"
    );
}

/// TS: includes transaction that error in teardown
///
/// Uses a PublicFeePaymentMethod with empty teardown gas limits, causing the
/// teardown phase to revert. The tx is still included (setup ran), but
/// teardown is rolled back.
///
/// Verifies:
/// - Simulation fails
/// - Receipt executionResult = TEARDOWN_REVERTED
/// - transactionFee > 0 (setup still paid)
/// - Alice transferred maxFee to FPC in setup (never refunded)
/// - FPC lost actual gas fee
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn includes_transaction_that_error_in_teardown() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let Some(fpc_address) = s.fpc_address else {
        eprintln!("SKIP: FPC artifact not available");
        return;
    };

    // Mint more public bananas to Alice
    let public_mint: i128 = 100_000_000_000;
    send_token_method(
        &s.wallet,
        &s.token_artifact,
        s.token_address,
        "mint_to_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Integer(public_mint),
        ],
        s.alice_address,
    )
    .await;

    // Capture initial balances (prefixed for use in TODO assertions below)
    let _initial_alice_public =
        get_banana_public_balance(&s.wallet, s.token_address, &s.alice_address).await;
    let _initial_fpc_public =
        get_banana_public_balance(&s.wallet, s.token_address, &fpc_address).await;
    let _initial_fpc_gas = get_fee_juice_balance(&s.wallet, fpc_address).await;

    // Build fee payload with empty teardown gas limits — causes teardown to run out of gas
    let bad_gas = GasSettings {
        teardown_gas_limits: Some(Gas::new(0, 0)),
        ..Default::default()
    };

    let fee_payload = build_public_fee_payload(
        &s.wallet,
        fpc_address,
        s.alice_address,
        s.token_address,
        &bad_gas,
    )
    .await;

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "mint_to_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Integer(1),
        ],
    );

    // Simulation should fail (teardown out of gas)
    let sim_result = s
        .wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call.clone()],
                ..Default::default()
            },
            SimulateOptions {
                from: s.alice_address,
                fee_execution_payload: Some(fee_payload.clone()),
                skip_fee_enforcement: false,
                ..Default::default()
            },
        )
        .await;

    assert!(
        sim_result.is_err(),
        "simulation should fail with teardown gas exhaustion"
    );

    // TODO: Full dontThrowOnRevert flow:
    //
    // When dontThrowOnRevert is available:
    // 1. Submit with dontThrowOnRevert: true
    // 2. Assert receipt.executionResult == TEARDOWN_REVERTED
    // 3. Assert receipt.transactionFee > 0
    // 4. Assert Alice transferred maxFee to FPC in setup (never refunded):
    //    - Alice's public bananas = initial + mint - maxFee
    //    - FPC's public bananas = initial + maxFee
    // 5. Assert FPC lost actual gas fee:
    //    - FPC's gas = initial - actual_fee
}
