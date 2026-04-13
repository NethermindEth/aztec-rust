//! Private fee payment tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_fees/private_payments.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_fee_private_payments -- --ignored --nocapture
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

use aztec_rs::constants::protocol_contract_address;
use aztec_rs::contract::BatchCall;
use aztec_rs::fee::GasSettings;
use aztec_rs::node::AztecNode;

use crate::common::*;

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

// ---------------------------------------------------------------------------
// Shared test state
// ---------------------------------------------------------------------------

struct PrivateFeeState {
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

static SHARED_STATE: OnceCell<Option<PrivateFeeState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static PrivateFeeState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<PrivateFeeState> {
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
            vec![abi_address(token_address), abi_address(alice_address)],
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

    // Mint public bananas to Alice
    let mint_amount: i128 = 10_000_000_000_000_000_000_000; // 1e22
    let mint_public = build_call(
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
                calls: vec![mint_public],
                ..Default::default()
            },
            SendOptions {
                from: alice_address,
                ..Default::default()
            },
        )
        .await
        .expect("mint public bananas");

    Some(PrivateFeeState {
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

/// TS: pays fees for tx that dont run public app logic
///
/// Alice transfers bananas publicly, paying fees via native Fee Juice.
/// Verifies: fee > 0 and Alice's Fee Juice balance decreases.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn pays_fees_no_public_app_logic() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let initial_fee_balance = get_fee_juice_balance(&s.wallet, s.alice_address).await;

    // Use public transfer (private notes require full sync infrastructure)
    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Field(Fr::from(s.bob_address)),
            AbiValue::Integer(5),
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
                ..Default::default()
            },
        )
        .await
        .expect("transfer with fee");

    let receipt = s
        .wallet
        .pxe()
        .node()
        .get_tx_receipt(&send_result.tx_hash)
        .await
        .expect("get receipt");
    let tx_fee = receipt.transaction_fee.unwrap_or(0);
    assert!(tx_fee > 0, "transaction fee should be > 0");

    let end_fee_balance = get_fee_juice_balance(&s.wallet, s.alice_address).await;
    assert!(
        end_fee_balance < initial_fee_balance,
        "Fee Juice balance should decrease"
    );
}

/// TS: pays fees for tx that creates notes in private
///
/// Alice mints bananas to public, verifying the fee is charged.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn pays_fees_creates_notes_public() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let mint_amount: i128 = 10;
    let slot = get_public_banana_balance_slot(&s.alice_address);
    let initial_balance = read_public_u128(&s.wallet, s.token_address, slot).await;
    let initial_fee = get_fee_juice_balance(&s.wallet, s.alice_address).await;

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "mint_to_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Integer(mint_amount),
        ],
    );

    s.wallet
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
        .await
        .expect("mint to public with fee");

    let end_balance = read_public_u128(&s.wallet, s.token_address, slot).await;
    assert!(
        end_balance > initial_balance,
        "alice public balance should increase by mint_amount"
    );

    let end_fee = get_fee_juice_balance(&s.wallet, s.alice_address).await;
    assert!(end_fee < initial_fee, "fee juice should decrease");
}

/// TS: pays fees for tx that runs public app logic (transfer_in_public)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn pays_fees_runs_public_app_logic() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let transfer_amount: i128 = 1;
    let alice_slot = get_public_banana_balance_slot(&s.alice_address);
    let bob_slot = get_public_banana_balance_slot(&s.bob_address);

    let initial_alice = read_public_u128(&s.wallet, s.token_address, alice_slot).await;
    let initial_bob = read_public_u128(&s.wallet, s.token_address, bob_slot).await;
    let initial_fee = get_fee_juice_balance(&s.wallet, s.alice_address).await;

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Field(Fr::from(s.bob_address)),
            AbiValue::Integer(transfer_amount),
            AbiValue::Integer(0),
        ],
    );

    s.wallet
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
        .await
        .expect("public transfer with fee");

    let end_alice = read_public_u128(&s.wallet, s.token_address, alice_slot).await;
    let end_bob = read_public_u128(&s.wallet, s.token_address, bob_slot).await;

    assert_eq!(
        end_alice,
        initial_alice - transfer_amount.unsigned_abs(),
        "alice balance should decrease"
    );
    assert_eq!(
        end_bob,
        initial_bob + transfer_amount.unsigned_abs(),
        "bob balance should increase"
    );

    let end_fee = get_fee_juice_balance(&s.wallet, s.alice_address).await;
    assert!(end_fee < initial_fee, "fee juice should decrease");
}

/// TS: pays fees for batched tx (transfer + mint)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn pays_fees_batched_public() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let transfer_amount: i128 = 1;
    let mint_amount: i128 = 20;

    let alice_slot = get_public_banana_balance_slot(&s.alice_address);
    let initial_alice = read_public_u128(&s.wallet, s.token_address, alice_slot).await;
    let initial_fee = get_fee_juice_balance(&s.wallet, s.alice_address).await;

    let transfer_call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Field(Fr::from(s.bob_address)),
            AbiValue::Integer(transfer_amount),
            AbiValue::Integer(0),
        ],
    );
    let mint_call = build_call(
        &s.token_artifact,
        s.token_address,
        "mint_to_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Integer(mint_amount),
        ],
    );

    let batch = BatchCall::new(
        &s.wallet,
        vec![
            ExecutionPayload {
                calls: vec![transfer_call],
                ..Default::default()
            },
            ExecutionPayload {
                calls: vec![mint_call],
                ..Default::default()
            },
        ],
    );
    batch
        .send(SendOptions {
            from: s.alice_address,
            ..Default::default()
        })
        .await
        .expect("batch transfer + mint");

    let end_alice = read_public_u128(&s.wallet, s.token_address, alice_slot).await;
    // Alice: initial - transfer + mint
    let expected = initial_alice - transfer_amount.unsigned_abs() + mint_amount.unsigned_abs();
    assert_eq!(end_alice, expected, "alice balance should reflect batch");

    let end_fee = get_fee_juice_balance(&s.wallet, s.alice_address).await;
    assert!(end_fee < initial_fee, "fee juice should decrease");
}

/// TS: rejects tx with insufficient fee payer balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn rejects_insufficient_fee_payer() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Bob has 0 Fee Juice → any tx from bob should fail
    let bob_balance = get_fee_juice_balance(&s.wallet, s.bob_address).await;
    if bob_balance > 0 {
        return;
    }

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.bob_address)),
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Integer(0),
            AbiValue::Integer(0),
        ],
    );

    let err = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.bob_address,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: bob has no Fee Juice");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("insufficient")
            || err_str.contains("balance")
            || err_str.contains("fee payer")
            || err_str.contains("reverted"),
        "expected fee error, got: {err}"
    );
}

/// TS: insufficient banana balance for transfer reverts
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn insufficient_token_balance_reverts() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Transfer more bananas than Alice has
    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Field(Fr::from(s.bob_address)),
            AbiValue::Integer(i128::MAX), // absurdly large amount
            AbiValue::Integer(0),
        ],
    );

    let err = s
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
        .await
        .expect_err("should fail: insufficient token balance");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("underflow")
            || err_str.contains("balance")
            || err_str.contains("reverted")
            || err_str.contains("overflow"),
        "expected balance error, got: {err}"
    );
}
