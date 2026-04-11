//! Nested private call tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_nested_contract/manual_private_call.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_nested_contract_manual_private_call -- --ignored --nocapture
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

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionSelector};
use aztec_rs::account::{SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
use aztec_rs::wallet::{BaseWallet, SendOptions};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn load_parent_contract_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/parent_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse parent_contract_compiled.json")
}

fn load_child_contract_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/child_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse child_contract_compiled.json")
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

async fn setup_wallet(account: ImportedTestAccount) -> Option<(TestWallet, AztecAddress)> {
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

    let secret_key = Fr::from_hex(account.secret_key).expect("valid test account secret key");
    let complete = imported_complete_address(account);

    if let Err(err) = pxe.key_store().add_account(&secret_key).await {
        eprintln!(
            "skipping: failed to seed key store for {}: {err}",
            account.alias
        );
        return None;
    }
    if let Err(err) = pxe.address_store().add(&complete).await {
        eprintln!(
            "skipping: failed to seed address store for {}: {err}",
            account.alias
        );
        return None;
    }

    let account_contract = SchnorrAccountContract::new(secret_key);
    let provider =
        SingleAccountProvider::new(complete.clone(), Box::new(account_contract), account.alias);
    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, complete.address))
}

// ---------------------------------------------------------------------------
// Contract interaction helpers
// ---------------------------------------------------------------------------

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

async fn deploy_contract(
    wallet: &TestWallet,
    artifact: ContractArtifact,
    from: AztecAddress,
) -> AztecAddress {
    let deploy = Contract::deploy(wallet, artifact, vec![], None).expect("deploy builder");
    let result = deploy
        .send(
            &DeployOptions {
                contract_address_salt: Some(Fr::from(next_unique_salt())),
                ..Default::default()
            },
            SendOptions {
                from,
                ..Default::default()
            },
        )
        .await
        .expect("deploy contract");
    result.instance.address
}

// ---------------------------------------------------------------------------
// Tests: manual_private_call
// ---------------------------------------------------------------------------

/// TS: performs nested calls
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn performs_nested_calls() {
    let _guard = serial_guard();

    let Some((wallet, default_account_address)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };

    let parent_artifact = load_parent_contract_artifact();
    let child_artifact = load_child_contract_artifact();

    let parent_address =
        deploy_contract(&wallet, parent_artifact.clone(), default_account_address).await;
    let child_address =
        deploy_contract(&wallet, child_artifact.clone(), default_account_address).await;

    let child_value_selector = child_artifact
        .find_function("value")
        .expect("find Child.value")
        .selector
        .expect("Child.value selector");

    let parent_contract = Contract::at(parent_address, parent_artifact, wallet);
    parent_contract
        .method(
            "entry_point",
            vec![
                abi_address(child_address),
                abi_selector(child_value_selector),
            ],
        )
        .expect("build Parent.entry_point call")
        .send(SendOptions {
            from: default_account_address,
            ..Default::default()
        })
        .await
        .expect("send Parent.entry_point");
}
