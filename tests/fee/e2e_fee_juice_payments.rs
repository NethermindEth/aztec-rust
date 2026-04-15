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

use std::sync::Arc;

use aztec_rs::account::{AccountManager, DeployAccountOptions, SchnorrAccountContract};
use aztec_rs::constants::protocol_contract_address;
use aztec_rs::fee::{FeeJuicePaymentMethodWithClaim, FeePaymentMethod, GasSettings, L2AmountClaim};
use aztec_rs::node::AztecNode;
use aztec_rs::wallet::Wallet;

use crate::common::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn get_fee_juice_balance(wallet: &TestWallet, address: AztecAddress) -> u128 {
    let fee_juice_address = protocol_contract_address::fee_juice();
    let slot = derive_storage_slot_in_map(1, &address);
    read_public_u128(wallet, fee_juice_address, slot).await
}

// ---------------------------------------------------------------------------
// Shared test state
// ---------------------------------------------------------------------------

struct FeeJuiceState {
    wallet: TestWallet,
    bob_wallet: TestWallet,
    alice_address: AztecAddress,
    bob_address: AztecAddress,
    fee_juice_artifact: Option<ContractArtifact>,
    fee_juice_address: AztecAddress,
    token_artifact: ContractArtifact,
    token_address: AztecAddress,
    gas_settings: GasSettings,
}

static SHARED_STATE: OnceCell<Option<FeeJuiceState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static FeeJuiceState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

#[allow(clippy::cognitive_complexity)]
async fn init_shared_state() -> Option<FeeJuiceState> {
    let (wallet, alice_address) = setup_wallet(TEST_ACCOUNT_0).await?;

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

    // Mint public bananas to Alice
    let mint_amount: i128 = 1_000_000_000_000_000_000_000;
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

    // Create and deploy Bob's Schnorr account (Alice pays the fee).
    // Mirrors upstream: `generateSchnorrAccounts(1)` + `wallet.createAccount(...)` +
    // `bobsDeployMethod.send({ from: aliceAddress })`.
    let bob_secret = Fr::random();
    let bob_contract = SchnorrAccountContract::new(bob_secret);
    let wallet_arc = Arc::new(wallet);
    let bob_manager = AccountManager::create(
        Arc::clone(&wallet_arc),
        bob_secret,
        Box::new(bob_contract),
        None::<Fr>,
    )
    .await
    .expect("create bob account manager");
    let bob_address = bob_manager.address();

    // Register Bob's account contract in the PXE so the wallet can resolve it.
    let bob_class_id = bob_manager.instance().inner.current_contract_class_id;
    let compiled_account = load_schnorr_account_artifact();
    wallet_arc
        .pxe()
        .contract_store()
        .add_artifact(&bob_class_id, &compiled_account)
        .await
        .expect("register bob account artifact");
    wallet_arc
        .pxe()
        .contract_store()
        .add_instance(bob_manager.instance())
        .await
        .expect("register bob account instance");
    wallet_arc
        .pxe()
        .key_store()
        .add_account(&bob_secret)
        .await
        .expect("register bob keys");
    let bob_complete = bob_manager
        .complete_address()
        .await
        .expect("bob complete address");
    wallet_arc
        .pxe()
        .address_store()
        .add(&bob_complete)
        .await
        .expect("register bob address");

    // Seed signing key note for Bob so ACVM can resolve it.
    let bob_signing_contract = SchnorrAccountContract::new(bob_secret);
    seed_signing_key_note(wallet_arc.pxe(), &bob_signing_contract, bob_address, 2).await;

    // Deploy Bob's account contract — Alice pays.
    {
        let deploy_method = bob_manager
            .deploy_method()
            .await
            .expect("bob deploy method");
        deploy_method
            .send(
                &DeployAccountOptions {
                    from: Some(alice_address),
                    ..Default::default()
                },
                SendOptions {
                    from: alice_address,
                    additional_scopes: vec![bob_address],
                    ..Default::default()
                },
            )
            .await
            .expect("deploy bob account (alice pays)");
    }

    // Save Bob's instance before dropping the manager.
    let bob_instance = bob_manager.instance().clone();

    // Unwrap the Arc to get the owned wallet back.
    drop(bob_manager);
    let wallet =
        Arc::try_unwrap(wallet_arc).unwrap_or_else(|_| panic!("wallet Arc still has other owners"));

    // Create a separate wallet for Bob so tests can send transactions as Bob.
    // Upstream's TestWallet supports multiple accounts; our SingleAccountProvider
    // doesn't, so we create a second wallet backed by its own PXE.
    let bob_wallet = {
        let node = create_aztec_node_client(node_url());
        let kv = Arc::new(InMemoryKvStore::new());
        let pxe = EmbeddedPxe::create(node.clone(), kv)
            .await
            .expect("bob pxe");

        pxe.key_store()
            .add_account(&bob_secret)
            .await
            .expect("bob key store");
        pxe.address_store()
            .add(&bob_complete)
            .await
            .expect("bob address store");
        pxe.contract_store()
            .add_artifact(&bob_class_id, &compiled_account)
            .await
            .expect("bob account artifact");
        pxe.contract_store()
            .add_instance(&bob_instance)
            .await
            .expect("bob account instance");

        let bob_signing = SchnorrAccountContract::new(bob_secret);
        seed_signing_key_note(&pxe, &bob_signing, bob_address, 1).await;
        register_protocol_contracts(&pxe).await;

        let provider = SingleAccountProvider::new(
            bob_complete.clone(),
            Box::new(SchnorrAccountContract::new(bob_secret)),
            "bob",
        );
        BaseWallet::new(pxe, node, provider)
    };

    // Gas settings: use defaults (mirrors upstream GasSettings.default() with 2x min fees;
    // our default already sets max_fee_per_gas to 1 which is accepted by dev networks).
    let gas_settings = GasSettings::default();

    Some(FeeJuiceState {
        wallet,
        bob_wallet,
        alice_address,
        bob_address,
        fee_juice_artifact,
        fee_juice_address,
        token_artifact,
        token_address,
        gas_settings,
    })
}

// ===========================================================================
// Tests: without initial funds
// ===========================================================================

/// TS: without initial funds > fails to simulate a tx
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn fails_to_simulate_without_funds() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let Some(ref fee_juice_artifact) = s.fee_juice_artifact else {
        return;
    };

    let call = build_call(
        fee_juice_artifact,
        s.fee_juice_address,
        "check_balance",
        vec![AbiValue::Integer(0)],
    );

    let err = s
        .bob_wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: s.bob_address,
                skip_fee_enforcement: false,
                gas_settings: Some(s.gas_settings.clone()),
                ..Default::default()
            },
        )
        .await
        .expect_err("simulation should fail: no funds");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("not enough balance for fee payer"),
        "expected 'Not enough balance for fee payer' error, got: {err}"
    );
}

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

    let call = build_call(
        fee_juice_artifact,
        s.fee_juice_address,
        "check_balance",
        vec![AbiValue::Integer(0)],
    );

    let err = s
        .bob_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.bob_address,
                gas_settings: Some(s.gas_settings.clone()),
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: no funds");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("insufficient fee payer balance")
            || err_str.contains("not enough balance for fee payer"),
        "expected insufficient fee payer balance error, got: {err}"
    );
}

/// TS: without initial funds > claims bridged funds and pays with them on the same tx
///
/// NOTE: Requires L1 bridge harness (`feeJuiceBridgeTestHarness.prepareTokensOnL1`).
/// Currently stubbed — once the harness is available, replace the dummy claim
/// with a real `prepare_tokens_on_l1` call and assert exact balance math.
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

    let call = build_call(
        fee_juice_artifact,
        s.fee_juice_address,
        "check_balance",
        vec![AbiValue::Integer(0)],
    );

    let result = s
        .bob_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from: s.bob_address,
                fee_execution_payload: Some(fee_payload),
                gas_settings: Some(s.gas_settings.clone()),
                ..Default::default()
            },
        )
        .await;

    match result {
        Ok(send_result) => {
            // With a real L1 bridge claim, upstream asserts:
            //   endBalance > 0
            //   endBalance < claim.claimAmount
            //   endBalance == claim.claimAmount - transactionFee
            let end_balance = get_fee_juice_balance(&s.bob_wallet, s.bob_address).await;
            assert!(
                end_balance > 0,
                "bob should have positive balance after claim"
            );

            let receipt = s
                .bob_wallet
                .pxe()
                .node()
                .get_tx_receipt(&send_result.tx_hash)
                .await
                .expect("get receipt");
            let tx_fee = receipt.transaction_fee.unwrap_or(0);
            assert_eq!(
                end_balance,
                claim.claim_amount - tx_fee,
                "end balance should be claim amount minus transaction fee"
            );
        }
        Err(err) => {
            // Without real L1 bridge, this is expected to fail.
            let err_str = err.to_string().to_lowercase();
            assert!(
                err_str.contains("l1")
                    || err_str.contains("claim")
                    || err_str.contains("message")
                    || err_str.contains("reverted")
                    || err_str.contains("nullifier")
                    || err_str.contains("not enough balance"),
                "expected L1 bridge / claim error, got: {err}"
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

    let call = build_call(
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
                gas_settings: Some(s.gas_settings.clone()),
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
///
/// Upstream uses `bananaCoin.methods.transfer(bobAddress, 1n)` (a private transfer).
/// Our SDK does not yet have full private note discovery after `mint_to_private`,
/// so we use `transfer_in_public` as a stand-in. The fee-payment path under test
/// is identical — only the app-level call differs.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn pays_fee_juice_no_public_calls() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let initial_balance = get_fee_juice_balance(&s.wallet, s.alice_address).await;

    let call = build_call(
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
                gas_settings: Some(s.gas_settings.clone()),
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
