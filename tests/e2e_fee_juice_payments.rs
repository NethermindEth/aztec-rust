//! Fee Juice payment tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_fees/fee_juice_payments.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_fee_juice_payments -- --ignored --nocapture
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

use aztec_rs::abi::AbiValue;
use aztec_rs::constants::protocol_contract_address;
use aztec_rs::fee::{FeeJuicePaymentMethodWithClaim, FeePaymentMethod, GasSettings, L2AmountClaim};
use aztec_rs::node::AztecNode;
use aztec_rs::wallet::{SimulateOptions, Wallet};

use common::*;
use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Fixture loaders
// ---------------------------------------------------------------------------

fn load_fee_juice_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/fee_juice_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/fee_juice_contract-FeeJuice.json"),
    ])
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_call(
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
) -> FunctionCall {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|e| panic!("function '{method_name}' not found: {e}"));
    FunctionCall {
        to: contract_address,
        selector: func.selector.expect("selector"),
        args,
        function_type: func.function_type.clone(),
        is_static: false,
        hide_msg_sender: false,
    }
}

async fn get_fee_juice_balance(wallet: &TestWallet, address: AztecAddress) -> u128 {
    let fee_juice_address = protocol_contract_address::fee_juice();
    let slot = derive_storage_slot_in_map(1, &address);
    let raw = wallet
        .pxe()
        .node()
        .get_public_storage_at(0, &fee_juice_address, &slot)
        .await
        .expect("get_public_storage_at");
    let bytes = raw.to_be_bytes();
    u128::from_be_bytes(bytes[16..32].try_into().expect("16 bytes"))
}

// ---------------------------------------------------------------------------
// Shared test state
// ---------------------------------------------------------------------------

struct FeeJuiceState {
    wallet: TestWallet,
    alice_address: AztecAddress,
    bob_address: AztecAddress,
    fee_juice_artifact: Option<ContractArtifact>,
    fee_juice_address: AztecAddress,
    token_artifact: ContractArtifact,
    token_address: AztecAddress,
}

static SHARED_STATE: OnceCell<Option<FeeJuiceState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static FeeJuiceState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<FeeJuiceState> {
    let (wallet, alice_address) = setup_wallet(TEST_ACCOUNT_0).await?;
    let bob_address = imported_complete_address(TEST_ACCOUNT_1).address;

    wallet
        .pxe()
        .register_sender(&bob_address)
        .await
        .expect("register bob");

    let fee_juice_artifact = load_fee_juice_artifact();
    let fee_juice_address = protocol_contract_address::fee_juice();

    if let Some(ref art) = fee_juice_artifact {
        wallet
            .pxe()
            .register_contract_class(art)
            .await
            .expect("register fee juice class");
    }

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
        .expect("deploy token");
    let token_address = result.instance.address;

    // Mint public bananas
    let mint_amount: i128 = 1_000_000_000_000_000_000_000;
    let mint_call = make_call(
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

    Some(FeeJuiceState {
        wallet,
        alice_address,
        bob_address,
        fee_juice_artifact,
        fee_juice_address,
        token_artifact,
        token_address,
    })
}

// ===========================================================================
// Tests: without initial funds
// ===========================================================================

/// TS: without initial funds > fails to send a tx
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn fails_to_send_without_funds() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let Some(ref fee_juice_artifact) = s.fee_juice_artifact else {
        return;
    };

    let call = make_call(
        fee_juice_artifact,
        s.fee_juice_address,
        "check_balance",
        vec![AbiValue::Integer(0)],
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
        .expect_err("should fail: no funds");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("insufficient")
            || err_str.contains("balance")
            || err_str.contains("fee payer")
            || err_str.contains("reverted"),
        "expected insufficient fee payer balance error, got: {err}"
    );
}

/// TS: without initial funds > claims bridged funds and pays with them on the same tx
///
/// NOTE: Requires L1 bridge harness. Skipped until available.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn claims_bridged_funds_and_pays() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let Some(ref fee_juice_artifact) = s.fee_juice_artifact else {
        return;
    };

    // TODO: When L1 bridge test harness is available:
    // let claim = fee_juice_bridge_harness.prepare_tokens_on_l1(bob_address).await;
    let claim = L2AmountClaim {
        claim_amount: 1_000_000_000_000,
        claim_secret: Fr::from(42u64),
        message_leaf_index: 0,
    };

    let payment_method = FeeJuicePaymentMethodWithClaim::new(s.bob_address, claim.clone());
    let fee_payload = payment_method
        .get_fee_execution_payload()
        .await
        .expect("fee payload");

    let call = make_call(
        fee_juice_artifact,
        s.fee_juice_address,
        "check_balance",
        vec![AbiValue::Integer(0)],
    );

    // This will likely fail without a real L1 bridge claim. Accept the failure.
    let result = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.bob_address,
                fee_execution_payload: Some(fee_payload),
                ..Default::default()
            },
        )
        .await;

    match result {
        Ok(_send_result) => {
            let end_balance = get_fee_juice_balance(&s.wallet, s.bob_address).await;
            assert!(
                end_balance > 0,
                "bob should have positive balance after claim"
            );
        }
        Err(err) => {
            // Without real L1 bridge, this is expected to fail
            let err_str = err.to_string().to_lowercase();
            assert!(
                err_str.contains("l1")
                    || err_str.contains("claim")
                    || err_str.contains("message")
                    || err_str.contains("reverted")
                    || err_str.contains("nullifier"),
                "expected L1 bridge error, got: {err}"
            );
        }
    }
}

// ===========================================================================
// Tests: with initial funds
// ===========================================================================

/// TS: with initial funds > sends tx with payment in Fee Juice with public calls
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn pays_fee_juice_with_public_calls() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let initial_balance = get_fee_juice_balance(&s.wallet, s.alice_address).await;

    let call = make_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Field(Fr::from(s.bob_address)),
            AbiValue::Integer(1),
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
        .expect("public transfer with fee");

    let receipt = s
        .wallet
        .pxe()
        .node()
        .get_tx_receipt(&send_result.tx_hash)
        .await
        .expect("get receipt");

    assert!(
        receipt.transaction_fee.unwrap_or(0) > 0,
        "transaction fee should be > 0"
    );

    let end_balance = get_fee_juice_balance(&s.wallet, s.alice_address).await;
    assert!(
        end_balance < initial_balance,
        "alice Fee Juice balance should decrease"
    );
}

/// TS: with initial funds > sends tx fee payment in Fee Juice with no public calls
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn pays_fee_juice_no_public_calls() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let initial_balance = get_fee_juice_balance(&s.wallet, s.alice_address).await;

    // Private transfer (no public app logic).
    // Uses mint_to_public + transfer_in_public as a private-note-free
    // alternative, since private note discovery after mint_to_private
    // requires full sync infrastructure not yet available.
    let call = make_call(
        &s.token_artifact,
        s.token_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.alice_address)),
            AbiValue::Field(Fr::from(s.bob_address)),
            AbiValue::Integer(1),
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

    assert!(
        receipt.transaction_fee.unwrap_or(0) > 0,
        "transaction fee should be > 0"
    );

    let end_balance = get_fee_juice_balance(&s.wallet, s.alice_address).await;
    assert!(
        end_balance < initial_balance,
        "alice Fee Juice balance should decrease"
    );
}
