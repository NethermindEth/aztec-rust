//! Sponsored fee payment tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_fees/sponsored_payments.test.ts`.
//!
//! Tests paying fees through a SponsoredFPC contract using the
//! `SponsoredFeePaymentMethod`, where the sponsor contract pays fees
//! unconditionally (no cost to the user).
//!
//! **Required fixture artifacts (compile from aztec-packages and place in `fixtures/`):**
//! - `sponsored_fpc_contract_compiled.json` (Sponsored Fee Payment Contract)
//! - `token_contract_compiled.json` (already present)
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_fee_sponsored_payments -- --ignored --nocapture
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
use aztec_rs::fee::{FeePaymentMethod, GasSettings, SponsoredFeePaymentMethod};
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

// ---------------------------------------------------------------------------
// Shared test state
// ---------------------------------------------------------------------------

struct SponsoredPaymentState {
    wallet: TestWallet,
    alice_address: AztecAddress,
    bob_address: AztecAddress,
    token_artifact: ContractArtifact,
    token_address: AztecAddress,
    sponsored_fpc_address: Option<AztecAddress>,
}

static SHARED_STATE: OnceCell<Option<SponsoredPaymentState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static SponsoredPaymentState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<SponsoredPaymentState> {
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

    // Deploy SponsoredFPC contract (optional — fixture may not be available)
    let sponsored_fpc_address = if let Some(sfpc_art) = load_sponsored_fpc_artifact() {
        let (sfpc_addr, _sfpc_art, _sfpc_instance) = deploy_contract(
            &wallet,
            sfpc_art,
            vec![], // SponsoredFPC has no constructor args
            alice_address,
        )
        .await;

        // NOTE: SponsoredFPC must be pre-funded with Fee Juice.
        // On a dev network this requires bridging gas from L1.

        Some(sfpc_addr)
    } else {
        None
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

    Some(SponsoredPaymentState {
        wallet,
        alice_address,
        bob_address,
        token_artifact,
        token_address,
        sponsored_fpc_address,
    })
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: pays fees for tx that makes a public transfer
///
/// Alice transfers 10 bananas to Bob using `SponsoredFeePaymentMethod`.
/// The SponsoredFPC pays the gas fee unconditionally. Alice pays zero gas.
///
/// Verifies:
/// - Alice loses only the transfer amount (no fee) in public bananas
/// - Bob gains the transfer amount
/// - SponsoredFPC's gas balance decreases by the fee
/// - Alice's gas balance is unchanged
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn pays_fees_for_tx_that_makes_a_public_transfer() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let Some(sfpc_address) = s.sponsored_fpc_address else {
        eprintln!(
            "SKIP: SponsoredFPC artifact not available — compile sponsored_fpc_contract_compiled.json"
        );
        return;
    };

    let bananas_to_send: u128 = 10;

    // Capture initial balances
    let initial_alice_bananas =
        get_banana_public_balance(&s.wallet, s.token_address, &s.alice_address).await;
    let initial_bob_bananas =
        get_banana_public_balance(&s.wallet, s.token_address, &s.bob_address).await;
    let initial_alice_gas = get_fee_juice_balance(&s.wallet, s.alice_address).await;
    let initial_sfpc_gas = get_fee_juice_balance(&s.wallet, sfpc_address).await;

    // Use SponsoredFeePaymentMethod — the SponsoredFPC pays unconditionally
    let payment = SponsoredFeePaymentMethod::new(sfpc_address);
    let fee_payload = payment
        .get_fee_execution_payload()
        .await
        .expect("fee payload");

    // Transfer bananas from Alice to Bob
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
        .expect("public transfer with sponsored fee");

    let receipt = s
        .wallet
        .pxe()
        .node()
        .get_tx_receipt(&send_result.tx_hash)
        .await
        .expect("get receipt");
    let fee_amount = receipt.transaction_fee.unwrap_or(0);
    assert!(fee_amount > 0, "transaction fee should be > 0");

    // Verify banana balances — Alice only loses the transfer amount, not the fee
    let end_alice_bananas =
        get_banana_public_balance(&s.wallet, s.token_address, &s.alice_address).await;
    let end_bob_bananas =
        get_banana_public_balance(&s.wallet, s.token_address, &s.bob_address).await;

    assert_eq!(
        end_alice_bananas,
        initial_alice_bananas - bananas_to_send,
        "alice should only lose the transfer amount (sponsor pays fee)"
    );
    assert_eq!(
        end_bob_bananas,
        initial_bob_bananas + bananas_to_send,
        "bob should gain the transfer amount"
    );

    // Verify gas balances — SponsoredFPC paid, not Alice
    let end_alice_gas = get_fee_juice_balance(&s.wallet, s.alice_address).await;
    let end_sfpc_gas = get_fee_juice_balance(&s.wallet, sfpc_address).await;

    assert_eq!(
        end_alice_gas, initial_alice_gas,
        "alice's gas should be unchanged (sponsor paid)"
    );
    assert_eq!(
        end_sfpc_gas,
        initial_sfpc_gas - u128::from(fee_amount),
        "SponsoredFPC's gas should decrease by fee"
    );
}
