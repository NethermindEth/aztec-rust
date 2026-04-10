//! Authentication witness tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_authwit.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_authwit -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::too_many_lines,
    dead_code,
    unused_imports
)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionSelector, FunctionType};
use aztec_rs::account::{AccountContract, SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::authwit::{lookup_validity, AuthWitValidity, SetPublicAuthWitInteraction};
use aztec_rs::constants::protocol_contract_address;
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::hash::{compute_inner_auth_wit_hash, MessageHashOrIntent};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::{Pxe, RegisterContractRequest};
use aztec_rs::tx::{AuthWitness, ExecutionPayload, FunctionCall};
use aztec_rs::types::{
    AztecAddress, CompleteAddress, ContractInstance, ContractInstanceWithAddress, Fr,
};
use aztec_rs::wallet::{BaseWallet, SendOptions, Wallet};

use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_auth_wit_test_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/auth_wit_test_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse auth_wit_test_contract_compiled.json")
}

fn load_generic_proxy_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/generic_proxy_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse generic_proxy_contract_compiled.json")
}

fn load_schnorr_account_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/schnorr_account_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse schnorr_account_contract_compiled.json")
}

// ---------------------------------------------------------------------------
// Setup helpers (mirrors upstream fixtures/utils.ts)
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
        "imported fixture address does not match derived complete address for {}",
        account.alias
    );
    complete
}

/// Register the compiled Schnorr account artifact on the PXE so that
/// utility functions (e.g. `lookup_validity`) can be executed locally.
///
/// The pre-imported accounts were deployed on the sandbox with a specific
/// class ID. We register the compiled artifact under that class ID so the
/// PXE has the bytecode needed for utility execution.
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

    // The pre-imported accounts were deployed with the dynamic artifact's
    // class ID. Compute it so we can store the compiled artifact (with
    // bytecode for lookup_validity) under the matching key.
    let class_id = aztec_rs::hash::compute_contract_class_id_from_artifact(&dynamic_artifact)
        .expect("compute class id");

    // Store the compiled artifact (with bytecode) under the dynamic class ID.
    pxe.contract_store()
        .add_artifact(&class_id, compiled_artifact)
        .await
        .expect("register compiled account artifact");

    // Store a contract instance so the PXE can look it up by address.
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

/// Create a wallet for `primary` account, registering the compiled Schnorr
/// artifact so `lookup_validity` works. Extra accounts are registered for
/// note discovery.
async fn create_wallet(
    primary: ImportedTestAccount,
    extra: &[ImportedTestAccount],
) -> Option<(TestWallet, AztecAddress)> {
    let url = node_url();
    let node = create_aztec_node_client(&url);
    if let Err(err) = node.get_node_info().await {
        eprintln!("skipping: node not reachable: {err}");
        return None;
    }

    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = match EmbeddedPxe::create(node.clone(), kv).await {
        Ok(pxe) => pxe,
        Err(err) => {
            eprintln!("skipping: failed to create PXE: {err}");
            return None;
        }
    };

    // Register primary account keys
    let secret_key = Fr::from_hex(primary.secret_key).expect("valid secret key");
    let complete = imported_complete_address(primary);
    pxe.key_store()
        .add_account(&secret_key)
        .await
        .expect("seed key store for primary");
    pxe.address_store()
        .add(&complete)
        .await
        .expect("seed address store for primary");

    // Register extra accounts
    for account in extra {
        let sk = Fr::from_hex(account.secret_key).expect("valid extra secret key");
        let ca = imported_complete_address(*account);
        pxe.key_store()
            .add_account(&sk)
            .await
            .expect("seed key store for extra");
        pxe.address_store()
            .add(&ca)
            .await
            .expect("seed address store for extra");
    }

    // Register compiled Schnorr account artifact on PXE for authwit utility.
    let compiled_account = load_schnorr_account_artifact();
    register_account_for_authwit(&pxe, &compiled_account, primary).await;
    for account in extra {
        register_account_for_authwit(&pxe, &compiled_account, *account).await;
    }

    let account_contract = SchnorrAccountContract::new(secret_key);

    // Seed the signing public key note into the PXE's note store.
    // The pre-imported accounts were deployed at genesis; their notes
    // aren't discoverable through standard sync.  We know the signing
    // key from the secret, so we insert it directly.
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
// Contract interaction helpers
// ---------------------------------------------------------------------------

/// Wait for the next block to ensure post-TX state is committed.
async fn wait_for_next_block(wallet: &TestWallet) {
    let current = wallet.pxe().node().get_block_number().await.unwrap_or(0);
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let now = wallet.pxe().node().get_block_number().await.unwrap_or(0);
        if now > current + 1 {
            return;
        }
    }
}

fn abi_address(address: AztecAddress) -> AbiValue {
    let mut fields = BTreeMap::new();
    fields.insert("inner".to_owned(), AbiValue::Field(Fr::from(address)));
    AbiValue::Struct(fields)
}

fn abi_selector(selector: FunctionSelector) -> AbiValue {
    let mut fields = BTreeMap::new();
    fields.insert(
        "inner".to_owned(),
        AbiValue::Integer(u32::from_be_bytes(selector.0).into()),
    );
    AbiValue::Struct(fields)
}

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

fn build_proxy_call(
    proxy_artifact: &ContractArtifact,
    proxy_address: AztecAddress,
    action: &FunctionCall,
) -> FunctionCall {
    let arg_count = action.args.len();
    let method_name = format!("forward_private_{arg_count}");
    let consume_selector = action.selector;

    build_call(
        proxy_artifact,
        proxy_address,
        &method_name,
        vec![
            abi_address(action.to),
            abi_selector(consume_selector),
            AbiValue::Array(
                action
                    .args
                    .iter()
                    .map(|a| match a {
                        AbiValue::Field(f) => AbiValue::Field(*f),
                        other => other.clone(),
                    })
                    .collect(),
            ),
        ],
    )
}

async fn register_contract_on_pxe(
    pxe: &impl Pxe,
    artifact: &ContractArtifact,
    instance: &ContractInstanceWithAddress,
) {
    pxe.register_contract_class(artifact)
        .await
        .unwrap_or_else(|e| eprintln!("register class: {e}"));
    pxe.register_contract(RegisterContractRequest {
        instance: instance.clone(),
        artifact: Some(artifact.clone()),
    })
    .await
    .expect("register contract");
}

// ---------------------------------------------------------------------------
// Shared test state (mirrors beforeAll)
// ---------------------------------------------------------------------------

struct TestState {
    wallet: TestWallet,
    account1: AztecAddress,
    account2: AztecAddress,
    auth_address: AztecAddress,
    auth_artifact: ContractArtifact,
    proxy_address: AztecAddress,
    proxy_artifact: ContractArtifact,
}

static SHARED_STATE: OnceCell<Option<TestState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static TestState> {
    let state = SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await;
    state.as_ref()
}

async fn init_shared_state() -> Option<TestState> {
    let (wallet, account1) = create_wallet(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1]).await?;
    let account2 =
        AztecAddress(Fr::from_hex(TEST_ACCOUNT_1.address).expect("valid account2 address"));

    eprintln!("account1: {account1}");
    eprintln!("account2: {account2}");

    let auth_artifact = load_auth_wit_test_artifact();
    let proxy_artifact = load_generic_proxy_artifact();

    // Deploy AuthWitTest contract from account1
    eprintln!("deploying AuthWitTest from account1...");
    let auth_deploy =
        Contract::deploy(&wallet, auth_artifact.clone(), vec![], None).expect("auth deploy");
    let auth_result = auth_deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: account1,
                ..Default::default()
            },
        )
        .await
        .expect("deploy AuthWitTest");
    let auth_address = auth_result.instance.address;
    eprintln!("AuthWitTest deployed at {auth_address}");

    // Deploy GenericProxy contract from account1
    eprintln!("deploying GenericProxy from account1...");
    let proxy_deploy =
        Contract::deploy(&wallet, proxy_artifact.clone(), vec![], None).expect("proxy deploy");
    let proxy_result = proxy_deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from: account1,
                ..Default::default()
            },
        )
        .await
        .expect("deploy GenericProxy");
    let proxy_address = proxy_result.instance.address;
    eprintln!("GenericProxy deployed at {proxy_address}");

    Some(TestState {
        wallet,
        account1,
        account2,
        auth_address,
        auth_artifact,
        proxy_address,
        proxy_artifact,
    })
}

// ---------------------------------------------------------------------------
// Tests: e2e_authwit — Private > arbitrary data
// ---------------------------------------------------------------------------

/// TS: Private > arbitrary data > happy path
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_authwit_arbitrary_data_happy_path() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Use a unique salt so different test runs don't collide on nullifiers.
    let inner_hash = compute_inner_auth_wit_hash(&[
        Fr::from_hex("0xdead").expect("valid hex"),
        Fr::from(next_unique_salt()),
    ]);

    let intent = MessageHashOrIntent::InnerHash {
        consumer: s.auth_address,
        inner_hash,
    };
    let witness = s
        .wallet
        .create_auth_wit(s.account1, intent.clone())
        .await
        .expect("create authwit");

    // Check validity for account1: private=true, public=false
    let validity = lookup_validity(&s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity for account1");
    assert_eq!(
        validity,
        AuthWitValidity {
            is_valid_in_private: true,
            is_valid_in_public: false,
        },
        "authwit should be valid in private for account1"
    );

    // Check NOT valid for account2: private=false, public=false
    let validity2 = lookup_validity(&s.wallet, &s.account2, &intent, &witness)
        .await
        .expect("lookup validity for account2");
    assert_eq!(
        validity2,
        AuthWitValidity {
            is_valid_in_private: false,
            is_valid_in_public: false,
        },
        "authwit should NOT be valid for account2"
    );

    // Consume via proxy
    let consume_action = build_call(
        &s.auth_artifact,
        s.auth_address,
        "consume",
        vec![abi_address(s.account1), AbiValue::Field(inner_hash)],
    );
    let proxy_call = build_proxy_call(&s.proxy_artifact, s.proxy_address, &consume_action);

    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![proxy_call],
                ..Default::default()
            },
            SendOptions {
                from: s.account1,
                auth_witnesses: vec![witness.clone()],
                ..Default::default()
            },
        )
        .await
        .expect("consume authwit via proxy");

    wait_for_next_block(&s.wallet).await;

    // Check validity after consumption: private=false, public=false
    let validity_after = lookup_validity(&s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity after consumption");
    assert_eq!(
        validity_after,
        AuthWitValidity {
            is_valid_in_private: false,
            is_valid_in_public: false,
        },
        "authwit should be invalid after consumption"
    );

    // Try to consume again — duplicate nullifier
    let consume_action2 = build_call(
        &s.auth_artifact,
        s.auth_address,
        "consume",
        vec![abi_address(s.account1), AbiValue::Field(inner_hash)],
    );
    let proxy_call2 = build_proxy_call(&s.proxy_artifact, s.proxy_address, &consume_action2);

    let err = s
        .wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![proxy_call2],
                ..Default::default()
            },
            SendOptions {
                from: s.account1,
                auth_witnesses: vec![witness],
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: duplicate nullifier");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("duplicate nullifier")
            || err_str.contains("nullifier already exists")
            || err_str.contains("nullifier collision")
            || err_str.contains("existing nullifier"),
        "expected duplicate nullifier error, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// Tests: e2e_authwit — Public > arbitrary data
// ---------------------------------------------------------------------------

/// TS: Public > arbitrary data > happy path
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_authwit_arbitrary_data_happy_path() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // Use a unique salt so different test runs don't collide on the
    // AuthRegistry's public storage (pre-funded accounts persist state).
    let inner_hash = compute_inner_auth_wit_hash(&[
        Fr::from_hex("0xdead").expect("valid hex"),
        Fr::from_hex("0x01").expect("valid hex"),
        Fr::from(next_unique_salt()),
    ]);

    let intent = MessageHashOrIntent::InnerHash {
        consumer: s.account2,
        inner_hash,
    };
    let witness = s
        .wallet
        .create_auth_wit(s.account1, intent.clone())
        .await
        .expect("create authwit");

    // Check validity: private=true, public=false
    let validity = lookup_validity(&s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity before set_public");
    assert_eq!(
        validity,
        AuthWitValidity {
            is_valid_in_private: true,
            is_valid_in_public: false,
        },
    );

    // Set public authwit (authorized=true)
    let set_public =
        SetPublicAuthWitInteraction::create(&s.wallet, s.account1, intent.clone(), true)
            .await
            .expect("create set_public");
    set_public
        .send(SendOptions::default())
        .await
        .expect("send set_public");

    wait_for_next_block(&s.wallet).await;

    // Check validity: private=true, public=true
    let validity_after_set = lookup_validity(&s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity after set_public");
    assert_eq!(
        validity_after_set,
        AuthWitValidity {
            is_valid_in_private: true,
            is_valid_in_public: true,
        },
    );

    // Consume via AuthRegistry.consume from account2
    let (wallet2, _) = create_wallet(TEST_ACCOUNT_1, &[TEST_ACCOUNT_0])
        .await
        .expect("create wallet for account2");

    let consume_call = FunctionCall {
        to: protocol_contract_address::auth_registry(),
        selector: FunctionSelector::from_signature("consume((Field),Field)"),
        args: vec![abi_address(s.account1), AbiValue::Field(inner_hash)],
        function_type: FunctionType::Public,
        is_static: false,
        hide_msg_sender: false,
    };
    wallet2
        .send_tx(
            ExecutionPayload {
                calls: vec![consume_call],
                ..Default::default()
            },
            SendOptions {
                from: s.account2,
                ..Default::default()
            },
        )
        .await
        .expect("consume public authwit from account2");

    // Wait for the consume TX's block to be fully committed before reading.
    wait_for_next_block(&wallet2).await;
    wait_for_next_block(&s.wallet).await;

    // Check validity: private=true, public=false (consumed in public)
    let validity_after_consume = lookup_validity(&s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity after consume");
    assert_eq!(
        validity_after_consume,
        AuthWitValidity {
            is_valid_in_private: true,
            is_valid_in_public: false,
        },
    );
}

/// TS: Public > arbitrary data > failure case > cancel before usage
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_authwit_cancel_before_usage() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let inner_hash = compute_inner_auth_wit_hash(&[
        Fr::from_hex("0xdead").expect("valid hex"),
        Fr::from_hex("0x02").expect("valid hex"),
        Fr::from(next_unique_salt()),
    ]);

    let intent = MessageHashOrIntent::InnerHash {
        consumer: s.auth_address,
        inner_hash,
    };
    let witness = s
        .wallet
        .create_auth_wit(s.account1, intent.clone())
        .await
        .expect("create authwit");

    // Check validity: private=true, public=false
    let validity = lookup_validity(&s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity");
    assert_eq!(
        validity,
        AuthWitValidity {
            is_valid_in_private: true,
            is_valid_in_public: false,
        },
    );

    // Set public authwit (authorized=true)
    let set_public =
        SetPublicAuthWitInteraction::create(&s.wallet, s.account1, intent.clone(), true)
            .await
            .expect("create set_public");
    set_public
        .send(SendOptions::default())
        .await
        .expect("send set_public");

    wait_for_next_block(&s.wallet).await;

    // Check validity: private=true, public=true
    let validity_set = lookup_validity(&s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity after set_public");
    assert_eq!(
        validity_set,
        AuthWitValidity {
            is_valid_in_private: true,
            is_valid_in_public: true,
        },
    );

    // Cancel public authwit (authorized=false)
    let cancel = SetPublicAuthWitInteraction::create(&s.wallet, s.account1, intent.clone(), false)
        .await
        .expect("create cancel");
    cancel
        .send(SendOptions::default())
        .await
        .expect("send cancel");

    wait_for_next_block(&s.wallet).await;

    // Check validity: private=true, public=false
    let validity_cancel = lookup_validity(&s.wallet, &s.account1, &intent, &witness)
        .await
        .expect("lookup validity after cancel");
    assert_eq!(
        validity_cancel,
        AuthWitValidity {
            is_valid_in_private: true,
            is_valid_in_public: false,
        },
    );

    // Try to consume via AuthRegistry — should fail with unauthorized
    let (wallet2, _) = create_wallet(TEST_ACCOUNT_1, &[TEST_ACCOUNT_0])
        .await
        .expect("create wallet for account2");

    // Try to consume via AuthRegistry — should fail.
    // Use send_tx (which includes node-side public simulation) rather than
    // simulate_tx (which only simulates the private part locally).
    let consume_call = FunctionCall {
        to: protocol_contract_address::auth_registry(),
        selector: FunctionSelector::from_signature("consume((Field),Field)"),
        args: vec![abi_address(s.account1), AbiValue::Field(inner_hash)],
        function_type: FunctionType::Public,
        is_static: false,
        hide_msg_sender: false,
    };
    let err = wallet2
        .send_tx(
            ExecutionPayload {
                calls: vec![consume_call],
                ..Default::default()
            },
            SendOptions {
                from: s.account2,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail: authwit was cancelled");

    let err_str = err.to_string().to_lowercase();
    assert!(
        err_str.contains("unauthorized")
            || err_str.contains("not authorized")
            || err_str.contains("assertion failed")
            || err_str.contains("reverted"),
        "expected unauthorized/reverted error, got: {err}"
    );
}
