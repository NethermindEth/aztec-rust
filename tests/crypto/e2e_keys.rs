//! Key management tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_keys.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_keys -- --ignored --nocapture
//! ```

#![allow(
    clippy::expect_used,
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
use aztec_rs::constants::domain_separator;
use aztec_rs::contract::Contract;
use aztec_rs::crypto::{
    complete_address_from_secret_key_and_partial_address, compute_app_nullifier_hiding_key,
    compute_ovsk_app, derive_master_nullifier_hiding_key,
    derive_master_outgoing_viewing_secret_key, derive_public_key_from_secret_key,
};
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::hash::{poseidon2_hash, poseidon2_hash_with_separator, silo_nullifier};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::Pxe;
use aztec_rs::tx::{ExecutionPayload, FunctionCall};
use aztec_rs::types::{
    AztecAddress, CompleteAddress, ContractInstance, ContractInstanceWithAddress, Fr,
};
use aztec_rs::wallet::{BaseWallet, SendOptions, SimulateOptions, Wallet};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_test_contract_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/test_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse test_contract_compiled.json")
}

fn load_schnorr_account_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/schnorr_account_contract_compiled.json");
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
        .expect("function not found in artifact");
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

/// Compute the hash of a Grumpkin Point: `poseidon2([x, y, is_infinite])`.
fn point_hash(x: Fr, y: Fr, is_infinite: bool) -> Fr {
    poseidon2_hash(&[x, y, Fr::from(is_infinite)])
}

/// Extract all note hashes and nullifiers from blocks `1..=current_block`.
///
/// Returns `(all_note_hashes, all_nullifiers)`.
#[allow(clippy::cognitive_complexity)]
async fn get_all_note_hashes_and_nullifiers(node: &HttpNodeClient) -> (Vec<Fr>, Vec<Fr>) {
    let current_block = node.get_block_number().await.expect("get block number");

    let mut all_note_hashes = Vec::new();
    let mut all_nullifiers = Vec::new();

    for bn in 1..=current_block {
        let Some(block) = node.get_block(bn).await.expect("get block") else {
            continue;
        };

        // block.body.txEffects[].noteHashes and .nullifiers
        let tx_effects = block.pointer("/body/txEffects").and_then(|v| v.as_array());

        if let Some(effects) = tx_effects {
            for effect in effects {
                if let Some(nhs) = effect.get("noteHashes").and_then(|v| v.as_array()) {
                    for nh in nhs {
                        if let Some(s) = nh.as_str() {
                            if let Ok(f) = Fr::from_hex(s) {
                                if !f.is_zero() {
                                    all_note_hashes.push(f);
                                }
                            }
                        }
                    }
                }
                if let Some(nulls) = effect.get("nullifiers").and_then(|v| v.as_array()) {
                    for n in nulls {
                        if let Some(s) = n.as_str() {
                            if let Ok(f) = Fr::from_hex(s) {
                                if !f.is_zero() {
                                    all_nullifiers.push(f);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    (all_note_hashes, all_nullifiers)
}

/// Count how many notes have been nullified, detectable via `nhk_app` alone.
///
/// For each note hash, derives the expected nullifier using `nhk_app` and checks
/// if it appears in the actual nullifiers from the chain.
async fn get_num_nullified_notes(
    node: &HttpNodeClient,
    nhk_app: &Fr,
    contract_address: &AztecAddress,
) -> usize {
    let (note_hashes, nullifiers) = get_all_note_hashes_and_nullifiers(node).await;

    let mut count = 0;
    for note_hash in &note_hashes {
        // inner_nullifier = poseidon2([noteHash, nhkApp], NOTE_NULLIFIER)
        let inner_nullifier = poseidon2_hash_with_separator(
            &[*note_hash, *nhk_app],
            domain_separator::NOTE_NULLIFIER,
        );
        // siloed_nullifier = silo_nullifier(contractAddress, innerNullifier)
        let siloed = silo_nullifier(contract_address, &inner_nullifier);

        if nullifiers.contains(&siloed) {
            count += 1;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Tests: Keys — using nhk_app to detect nullification
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn nhk_app_detects_note_nullification() {
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

    // Derive nhk_app from the account secret key
    let secret_key = Fr::from_hex(TEST_ACCOUNT_0.secret_key).expect("valid secret key");
    let nhk_m = derive_master_nullifier_hiding_key(&secret_key);
    let nhk_app = compute_app_nullifier_hiding_key(&nhk_m, &contract_address);

    let note_value = Fr::from(5u64);
    let note_storage_slot = Fr::from(12u64);

    // call_create_note(value, owner, storage_slot, broadcast)
    let create_call = build_call(
        &artifact,
        contract_address,
        "call_create_note",
        vec![
            AbiValue::Field(note_value),
            AbiValue::Field(default_account.0),
            AbiValue::Field(note_storage_slot),
            AbiValue::Boolean(false),
        ],
    );
    wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![create_call],
                ..Default::default()
            },
            SendOptions {
                from: default_account,
                ..Default::default()
            },
        )
        .await
        .expect("call_create_note should succeed");

    // Before destroying: 0 nullified notes detectable via nhk_app
    let count_before = get_num_nullified_notes(wallet.node(), &nhk_app, &contract_address).await;
    assert_eq!(
        count_before, 0,
        "expected 0 nullified notes before destroy, got {count_before}"
    );

    // call_destroy_note(owner, storage_slot)
    let destroy_call = build_call(
        &artifact,
        contract_address,
        "call_destroy_note",
        vec![
            AbiValue::Field(default_account.0),
            AbiValue::Field(note_storage_slot),
        ],
    );
    wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![destroy_call],
                ..Default::default()
            },
            SendOptions {
                from: default_account,
                ..Default::default()
            },
        )
        .await
        .expect("call_destroy_note should succeed");

    // After destroying: 1 nullified note detectable via nhk_app
    let count_after = get_num_nullified_notes(wallet.node(), &nhk_app, &contract_address).await;
    assert_eq!(
        count_after, 1,
        "expected 1 nullified note after destroy, got {count_after}"
    );
}

// ---------------------------------------------------------------------------
// Tests: Keys — ovsk_app
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn gets_ovsk_app() {
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

    // Derive ovsk_m and ovpk_m from the account secret key
    let secret_key = Fr::from_hex(TEST_ACCOUNT_0.secret_key).expect("valid secret key");
    let ovsk_m = derive_master_outgoing_viewing_secret_key(&secret_key);
    let ovpk_m = derive_public_key_from_secret_key(&ovsk_m);
    let ovpk_m_hash = point_hash(ovpk_m.x, ovpk_m.y, ovpk_m.is_infinite);

    // Compute the expected ovsk_app
    let expected_ovsk_app = compute_ovsk_app(&ovsk_m, &contract_address);

    // Simulate get_ovsk_app(ovpk_m_hash) on the test contract
    let call = build_call(
        &artifact,
        contract_address,
        "get_ovsk_app",
        vec![AbiValue::Field(ovpk_m_hash)],
    );
    let result = wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from: default_account,
                ..Default::default()
            },
        )
        .await
        .expect("simulate get_ovsk_app");

    // Extract the return value from the simulation result
    let ovsk_app_str = result
        .return_values
        .pointer("/returnValues/0")
        .and_then(|v| v.as_str())
        .expect("get_ovsk_app should return a field element");
    let ovsk_app = Fr::from_hex(ovsk_app_str).expect("parse ovsk_app as Fr");

    // The ovsk_app from the contract should match our local derivation.
    // compute_ovsk_app returns GrumpkinScalar (Fq), but the contract returns Fr.
    // Convert the expected value to Fr for comparison.
    let expected_as_fr = Fr::from(expected_ovsk_app.to_be_bytes());

    assert_eq!(
        ovsk_app, expected_as_fr,
        "ovsk_app from contract does not match local derivation"
    );
}
