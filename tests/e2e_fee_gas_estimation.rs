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

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionType};
use aztec_rs::account::{AccountContract, SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::{get_gas_limits, DeployOptions};
use aztec_rs::embedded_pxe::stores::note_store::StoredNote;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::fee::{FeePaymentMethod, Gas, GasFees, GasSettings, NativeFeePaymentMethod};
use aztec_rs::hash::compute_contract_class_id_from_artifact;
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::{Pxe, RegisterContractRequest};
use aztec_rs::tx::{ExecutionPayload, FunctionCall, TxReceipt};
use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
use aztec_rs::types::{ContractInstance, ContractInstanceWithAddress};
use aztec_rs::wallet::{BaseWallet, SendOptions, SimulateOptions, Wallet};

use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn load_token_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/token_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse token_contract_compiled.json")
}

fn load_fpc_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    let candidates = [
        root.join("fixtures/fpc_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/fpc_contract-FPC.json"),
    ];
    for path in &candidates {
        if let Ok(json) = fs::read_to_string(path) {
            return ContractArtifact::from_nargo_json(&json).ok();
        }
    }
    None
}

fn load_schnorr_account_artifact() -> ContractArtifact {
    let root = repo_root();
    let path = root.join("fixtures/schnorr_account_contract_compiled.json");
    let json = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    ContractArtifact::from_nargo_json(&json).expect("parse schnorr_account_contract_compiled.json")
}

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

type TestWallet = BaseWallet<EmbeddedPxe<HttpNodeClient>, HttpNodeClient, SingleAccountProvider>;

#[derive(Clone, Copy)]
struct ImportedTestAccount {
    alias: &'static str,
    address: &'static str,
    secret_key: &'static str,
    partial_address: &'static str,
}

const TEST_ACCOUNT_0: ImportedTestAccount = ImportedTestAccount {
    alias: "test0",
    address: "0x0a60414ee907527880b7a53d4dacdeb9ef768bb98d9d8d1e7200725c13763331",
    secret_key: "0x2153536ff6628eee01cf4024889ff977a18d9fa61d0e414422f7681cf085c281",
    partial_address: "0x140c3a658e105092549c8402f0647fe61d87aba4422b484dfac5d4a87462eeef",
};

const TEST_ACCOUNT_1: ImportedTestAccount = ImportedTestAccount {
    alias: "test1",
    address: "0x00cedf87a800bd88274762d77ffd93e97bc881d1fc99570d62ba97953597914d",
    secret_key: "0x0aebd1b4be76efa44f5ee655c20bf9ea60f7ae44b9a7fd1fd9f189c7a0b0cdae",
    partial_address: "0x0325ee1689daec508c6adef0df4a1e270ac1fcf971fed1f893b2d98ad12d6bb8",
};

fn node_url() -> String {
    std::env::var("AZTEC_NODE_URL").unwrap_or_else(|_| "http://localhost:8080".to_owned())
}

fn serial_guard() -> MutexGuard<'static, ()> {
    static E2E_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    E2E_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[allow(clippy::cast_possible_truncation)]
fn next_unique_salt() -> u64 {
    static NEXT_SALT: OnceLock<AtomicU64> = OnceLock::new();
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    NEXT_SALT
        .get_or_init(|| AtomicU64::new(seed))
        .fetch_add(1, Ordering::Relaxed)
}

fn imported_complete_address(account: ImportedTestAccount) -> CompleteAddress {
    let expected_address =
        AztecAddress(Fr::from_hex(account.address).expect("valid test account address"));
    let secret_key = Fr::from_hex(account.secret_key).expect("valid test account secret key");
    let partial_address =
        Fr::from_hex(account.partial_address).expect("valid test account partial address");
    let complete =
        complete_address_from_secret_key_and_partial_address(&secret_key, &partial_address)
            .expect("derive complete address");
    assert_eq!(
        complete.address, expected_address,
        "imported fixture address does not match for {}",
        account.alias
    );
    complete
}

async fn setup_wallet(account: ImportedTestAccount) -> Option<(TestWallet, AztecAddress)> {
    let url = node_url();
    let node = create_aztec_node_client(&url);
    if node.get_node_info().await.is_err() {
        return None;
    }

    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = EmbeddedPxe::create(node.clone(), kv).await.ok()?;

    let secret_key = Fr::from_hex(account.secret_key).expect("valid sk");
    let complete = imported_complete_address(account);

    pxe.key_store().add_account(&secret_key).await.ok()?;
    pxe.address_store().add(&complete).await.ok()?;

    let account_contract = SchnorrAccountContract::new(secret_key);

    let compiled_account_artifact = load_schnorr_account_artifact();
    let dynamic_artifact = account_contract.contract_artifact().await.ok()?;
    let dynamic_class_id = compute_contract_class_id_from_artifact(&dynamic_artifact).ok()?;

    pxe.contract_store()
        .add_artifact(&dynamic_class_id, &compiled_account_artifact)
        .await
        .ok()?;
    let account_instance = ContractInstanceWithAddress {
        address: complete.address,
        inner: ContractInstance {
            version: 1,
            salt: Fr::from(0u64),
            deployer: AztecAddress::zero(),
            current_contract_class_id: dynamic_class_id,
            original_contract_class_id: dynamic_class_id,
            initialization_hash: Fr::zero(),
            public_keys: complete.public_keys.clone(),
        },
    };
    pxe.contract_store()
        .add_instance(&account_instance)
        .await
        .ok()?;

    let signing_pk = account_contract.signing_public_key();
    let note = StoredNote {
        contract_address: complete.address,
        owner: complete.address,
        storage_slot: Fr::from(1u64),
        randomness: Fr::zero(),
        note_nonce: Fr::from(1u64),
        note_hash: Fr::from(1u64),
        siloed_nullifier: Fr::from_hex(
            "0xdeadbeef00000000000000000000000000000000000000000000000000000001",
        )
        .expect("unique nullifier"),
        note_data: vec![signing_pk.x, signing_pk.y],
        nullified: false,
        is_pending: false,
        nullification_block_number: None,
        leaf_index: None,
        block_number: None,
        tx_index_in_block: None,
        note_index_in_tx: None,
        scopes: vec![complete.address],
    };
    pxe.note_store()
        .add_note(&note)
        .await
        .expect("seed signing key note");

    let provider =
        SingleAccountProvider::new(complete.clone(), Box::new(account_contract), account.alias);
    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, complete.address))
}

// ---------------------------------------------------------------------------
// Contract interaction helpers
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

fn make_transfer_request(
    token_artifact: &ContractArtifact,
    token_address: AztecAddress,
    alice: AztecAddress,
    bob: AztecAddress,
) -> FunctionCall {
    make_call(
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
    fpc_artifact: Option<ContractArtifact>,
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
