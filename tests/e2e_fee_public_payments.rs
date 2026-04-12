//! Public fee payment tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_fees/public_payments.test.ts`.
//!
//! Tests paying fees through a Fee Payment Contract (FPC) using the
//! public fee payment method, where Alice transfers bananas to FPC in setup
//! and FPC pays gas fees.
//!
//! **Required fixture artifacts (compile from aztec-packages and place in `fixtures/`):**
//! - `fpc_contract_compiled.json` (Fee Payment Contract)
//! - `token_contract_compiled.json` (already present)
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_fee_public_payments -- --ignored --nocapture
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

use aztec_rs::abi::FunctionSelector;
use aztec_rs::authwit::SetPublicAuthWitInteraction;
use aztec_rs::constants::protocol_contract_address;
use aztec_rs::fee::{FeePaymentMethod, GasSettings};
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

// Token storage: admin(1), minters(2), balances(3), total_supply(4), public_balances(5)
fn get_public_banana_balance_slot(address: &AztecAddress) -> Fr {
    derive_storage_slot_in_map(5, address) // public_balances at slot 5
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
/// maxFee = (gas_limits.da * fee_per_da) + (gas_limits.l2 * fee_per_l2)
///        + (teardown_limits.da * fee_per_da) + (teardown_limits.l2 * fee_per_l2)
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
///
/// Mirrors TS `PublicFeePaymentMethod.getExecutionPayload()`:
/// 1. Set public authwit allowing FPC to call `transfer_in_public(sender, fpc, maxFee, nonce)` on token
/// 2. Call `fee_entrypoint_public(maxFee, nonce)` on the FPC
/// 3. Set fee_payer to FPC address
async fn build_public_fee_payload(
    wallet: &TestWallet,
    fpc_address: AztecAddress,
    sender: AztecAddress,
    token_address: AztecAddress,
    gas_settings: &GasSettings,
) -> ExecutionPayload {
    let max_fee = compute_max_fee(gas_settings);
    let nonce = Fr::random();

    // Construct the token transfer call that the FPC will make on behalf of the sender
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

    // Create authwit allowing FPC to call transfer_in_public on the token
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

    // Build the FPC entrypoint call
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

    // Combine: authwit calls + FPC entrypoint, with FPC as fee payer
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

struct PublicPaymentState {
    wallet: TestWallet,
    alice_address: AztecAddress,
    bob_address: AztecAddress,
    token_artifact: ContractArtifact,
    token_address: AztecAddress,
    fpc_artifact: Option<ContractArtifact>,
    fpc_address: Option<AztecAddress>,
}

static SHARED_STATE: OnceCell<Option<PublicPaymentState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static PublicPaymentState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<PublicPaymentState> {
    let (wallet, alice_address) = setup_wallet(TEST_ACCOUNT_0).await?;
    let bob_address = imported_complete_address(TEST_ACCOUNT_1).address;

    wallet
        .pxe()
        .register_sender(&bob_address)
        .await
        .expect("register bob");

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

    // Deploy FPC contract (optional — fixture may not be available)
    let (fpc_artifact, fpc_address) = if let Some(fpc_art) = load_fpc_artifact() {
        let (fpc_addr, fpc_art, _fpc_instance) = deploy_contract(
            &wallet,
            fpc_art,
            vec![abi_address(token_address), abi_address(alice_address)],
            alice_address,
        )
        .await;

        // Bridge Fee Juice to FPC so it can pay gas
        // NOTE: Without L1 bridge harness, FPC must have Fee Juice pre-funded.
        // On a dev network with funded test accounts, the FPC may need separate funding.

        (Some(fpc_art), Some(fpc_addr))
    } else {
        (None, None)
    };

    // Mint public bananas to Alice (1e22 = upstream ALICE_INITIAL_BANANAS)
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

    // Also mint private bananas to Alice
    mint_tokens_to_private(
        &wallet,
        token_address,
        &token_artifact,
        alice_address,
        alice_address,
        10_000_000,
    )
    .await;

    Some(PublicPaymentState {
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

/// TS: pays fees for tx that make public transfer
///
/// Alice transfers 10 bananas to Bob, paying fees through the FPC using
/// the public fee payment method. Verifies:
/// - Alice's public banana balance decreases by fee + transfer amount
/// - FPC's public banana balance increases by fee
/// - Bob's public banana balance increases by transfer amount
/// - FPC's gas balance decreases by fee
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn pays_fees_for_tx_that_make_public_transfer() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let (Some(ref _fpc_artifact), Some(fpc_address)) = (&s.fpc_artifact, s.fpc_address) else {
        eprintln!("SKIP: FPC artifact not available — compile fpc_contract_compiled.json");
        return;
    };

    let bananas_to_send: u128 = 10;

    // Capture initial balances
    let initial_alice_bananas =
        get_banana_public_balance(&s.wallet, s.token_address, &s.alice_address).await;
    let initial_bob_bananas =
        get_banana_public_balance(&s.wallet, s.token_address, &s.bob_address).await;
    let initial_fpc_bananas =
        get_banana_public_balance(&s.wallet, s.token_address, &fpc_address).await;
    let initial_alice_gas = get_fee_juice_balance(&s.wallet, s.alice_address).await;
    let initial_fpc_gas = get_fee_juice_balance(&s.wallet, fpc_address).await;

    // Build the public fee payment payload
    let fee_payload = build_public_fee_payload(
        &s.wallet,
        fpc_address,
        s.alice_address,
        s.token_address,
        &GasSettings::default(),
    )
    .await;

    // Transfer bananas from Alice to Bob, paying fees via FPC
    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Field(Fr::from(s.bob_address)),
            AbiValue::Integer(i128::try_from(bananas_to_send).expect("safe cast")),
            AbiValue::Integer(0),
        ],
    );

    let send_result = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.alice_address,
                fee_execution_payload: Some(fee_payload),
                ..Default::default()
            },
        )
        .await
        .expect("public transfer with FPC fee");

    let receipt = s
        .wallet
        .pxe()
        .node()
        .get_tx_receipt(&send_result.tx_hash)
        .await
        .expect("get receipt");
    let fee_amount = receipt.transaction_fee.unwrap_or(0);
    assert!(fee_amount > 0, "transaction fee should be > 0");

    // Verify banana balances
    let end_alice_bananas =
        get_banana_public_balance(&s.wallet, s.token_address, &s.alice_address).await;
    let end_bob_bananas =
        get_banana_public_balance(&s.wallet, s.token_address, &s.bob_address).await;
    let end_fpc_bananas = get_banana_public_balance(&s.wallet, s.token_address, &fpc_address).await;

    assert_eq!(
        end_alice_bananas,
        initial_alice_bananas - u128::from(fee_amount) - bananas_to_send,
        "alice should lose fee + transfer amount in public bananas"
    );
    assert_eq!(
        end_fpc_bananas,
        initial_fpc_bananas + u128::from(fee_amount),
        "FPC should gain the fee in public bananas"
    );
    assert_eq!(
        end_bob_bananas,
        initial_bob_bananas + bananas_to_send,
        "bob should gain the transfer amount"
    );

    // Verify gas balances (FPC paid gas, not Alice)
    let end_alice_gas = get_fee_juice_balance(&s.wallet, s.alice_address).await;
    let end_fpc_gas = get_fee_juice_balance(&s.wallet, fpc_address).await;

    assert_eq!(
        end_alice_gas, initial_alice_gas,
        "alice's gas should be unchanged (FPC paid)"
    );
    assert_eq!(
        end_fpc_gas,
        initial_fpc_gas - u128::from(fee_amount),
        "FPC's gas should decrease by fee"
    );
}
