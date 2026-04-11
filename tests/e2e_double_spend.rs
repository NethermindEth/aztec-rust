//! Double spend tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_double_spend.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_double_spend -- --ignored --nocapture
//! ```

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::too_many_lines,
    dead_code,
    unused_imports
)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionType};
use aztec_rs::account::{AccountContract, SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::Pxe;
use aztec_rs::tx::{ExecutionPayload, FunctionCall};
use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
use aztec_rs::wallet::{BaseWallet, SendOptions, SimulateOptions, Wallet};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_test_contract_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/test_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse test_contract_compiled.json")
}

fn load_schnorr_account_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/schnorr_account_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse schnorr_account_contract_compiled.json")
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

fn node_url() -> String {
    std::env::var("AZTEC_NODE_URL").unwrap_or_else(|_| "http://localhost:8080".to_owned())
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
        "imported fixture address does not match derived complete address for {}",
        account.alias
    );
    complete
}

use aztec_rs::types::{ContractInstance, ContractInstanceWithAddress};

async fn register_account_for_authwit(
    pxe: &EmbeddedPxe<HttpNodeClient>,
    compiled_artifact: &ContractArtifact,
    account: ImportedTestAccount,
) {
    let secret_key = Fr::from_hex(account.secret_key).expect("valid sk");
    let account_contract = SchnorrAccountContract::new(secret_key);
    let dynamic_artifact = account_contract
        .contract_artifact()
        .await
        .expect("dynamic artifact");
    let complete = imported_complete_address(account);

    let class_id = aztec_rs::hash::compute_contract_class_id_from_artifact(&dynamic_artifact)
        .expect("compute class id");

    pxe.contract_store()
        .add_artifact(&class_id, compiled_artifact)
        .await
        .expect("register compiled account artifact");

    let instance = ContractInstanceWithAddress {
        address: complete.address,
        inner: ContractInstance {
            version: 1,
            salt: Fr::from(0u64),
            deployer: AztecAddress::zero(),
            current_contract_class_id: class_id,
            original_contract_class_id: class_id,
            initialization_hash: Fr::zero(),
            public_keys: complete.public_keys.clone(),
        },
    };
    pxe.contract_store()
        .add_instance(&instance)
        .await
        .expect("register account instance");
}

async fn create_wallet(primary: ImportedTestAccount) -> Option<(TestWallet, AztecAddress)> {
    let url = node_url();
    let node = create_aztec_node_client(&url);
    if let Err(_err) = node.get_node_info().await {
        return None;
    }

    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = match EmbeddedPxe::create(node.clone(), kv).await {
        Ok(pxe) => pxe,
        Err(_err) => {
            return None;
        }
    };

    let secret_key = Fr::from_hex(primary.secret_key).expect("valid secret key");
    let complete = imported_complete_address(primary);
    pxe.key_store()
        .add_account(&secret_key)
        .await
        .expect("seed key store");
    pxe.address_store()
        .add(&complete)
        .await
        .expect("seed address store");

    let compiled_account = load_schnorr_account_artifact();
    register_account_for_authwit(&pxe, &compiled_account, primary).await;

    let account_contract = SchnorrAccountContract::new(secret_key);

    // Seed the signing public key note into the PXE's note store.
    let signing_pk = account_contract.signing_public_key();
    let note = aztec_rs::embedded_pxe::stores::note_store::StoredNote {
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
        SingleAccountProvider::new(complete.clone(), Box::new(account_contract), primary.alias);
    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, complete.address))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_call(
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
) -> FunctionCall {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|_| panic!("function '{method_name}' not found in artifact"));
    let selector = func.selector.expect("selector");
    FunctionCall {
        to: contract_address,
        selector,
        args,
        function_type: func.function_type.clone(),
        is_static: func.is_static,
        hide_msg_sender: false,
    }
}

// ---------------------------------------------------------------------------
// Tests: e2e_double_spend
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn emits_public_nullifier_then_tries_same_nullifier() {
    let (wallet, default_account) = create_wallet(TEST_ACCOUNT_0).await.expect("wallet setup");

    let artifact = load_test_contract_artifact();

    // Deploy TestContract
    let deploy = Contract::deploy(&wallet, artifact.clone(), vec![], None).expect("deploy setup");
    let deploy_result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: default_account,
                ..Default::default()
            },
        )
        .await
        .expect("deploy TestContract");
    let contract_address = deploy_result.instance.address;

    // Use a unique nullifier value to avoid collisions across test runs
    let nullifier = Fr::from(1u64);

    // TX1: emit a public nullifier — should succeed
    let call1 = build_call(
        &artifact,
        contract_address,
        "emit_nullifier_public",
        vec![AbiValue::Field(nullifier)],
    );
    wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call1],
                ..Default::default()
            },
            SendOptions {
                from: default_account,
                ..Default::default()
            },
        )
        .await
        .expect("TX1: emit_nullifier_public should succeed");

    // TX2-simulate: try emitting the same nullifier — simulation should fail
    let call2 = build_call(
        &artifact,
        contract_address,
        "emit_nullifier_public",
        vec![AbiValue::Field(nullifier)],
    );
    let sim_err = wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call2],
                ..Default::default()
            },
            SimulateOptions {
                from: default_account,
                ..Default::default()
            },
        )
        .await
        .expect_err("TX2-simulate: should fail with duplicate nullifier");

    let sim_err_str = sim_err.to_string().to_lowercase();
    assert!(
        sim_err_str.contains("duplicate nullifier")
            || sim_err_str.contains("nullifier already exist")
            || sim_err_str.contains("nullifier collision")
            || sim_err_str.contains("existing nullifier")
            || sim_err_str.contains("reverted"),
        "expected duplicate-nullifier or revert error from simulation, got: {sim_err}"
    );

    // TX2-send: try sending (skipping simulation) — should fail with revert
    let call3 = build_call(
        &artifact,
        contract_address,
        "emit_nullifier_public",
        vec![AbiValue::Field(nullifier)],
    );
    let send_err = wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call3],
                ..Default::default()
            },
            SendOptions {
                from: default_account,
                ..Default::default()
            },
        )
        .await
        .expect_err("TX2-send: should fail with app logic reverted");

    let send_err_str = send_err.to_string().to_lowercase();
    assert!(
        send_err_str.contains("reverted")
            || send_err_str.contains("duplicate nullifier")
            || send_err_str.contains("nullifier already exist")
            || send_err_str.contains("nullifier collision")
            || send_err_str.contains("existing nullifier")
            || send_err_str.contains("rejected"),
        "expected revert/duplicate-nullifier error from send, got: {send_err}"
    );
}
