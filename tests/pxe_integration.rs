//! Integration tests for the PXE client against a live Aztec PXE.
//!
//! These tests are `#[ignore]`d by default because they require a running
//! PXE instance. Run them with:
//!
//! ```bash
//! AZTEC_PXE_URL=http://localhost:8080 cargo test --test pxe_integration -- --ignored
//! ```

#![allow(clippy::expect_used, clippy::print_stderr)]

use aztec_rs::pxe::{create_pxe_client, Pxe};
use aztec_rs::types::{AztecAddress, Fr};

fn pxe_url() -> String {
    std::env::var("AZTEC_PXE_URL").unwrap_or_else(|_| "http://localhost:8080".to_owned())
}

async fn require_live_pxe() -> Option<impl Pxe> {
    let pxe = create_pxe_client(pxe_url());
    // Single attempt — fail fast instead of retrying for 120s per test.
    match pxe.get_synced_block_header().await {
        Ok(_) => Some(pxe),
        Err(err) => {
            eprintln!("skipping: PXE not reachable: {err}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Connectivity
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a reachable PXE via AZTEC_PXE_URL"]
async fn pxe_get_synced_block_header() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };
    let header = pxe
        .get_synced_block_header()
        .await
        .expect("get synced block header");
    // The header is opaque JSON; just verify it's a non-null object.
    assert!(
        header.data.is_object(),
        "block header should be a JSON object"
    );
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a reachable PXE via AZTEC_PXE_URL"]
async fn pxe_get_registered_accounts() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };
    // Should succeed even if no accounts are registered yet.
    let accounts = pxe
        .get_registered_accounts()
        .await
        .expect("get registered accounts");
    // Sandbox pre-registers test accounts; any count is valid.
    eprintln!("registered accounts: {}", accounts.len());
}

#[tokio::test]
#[ignore = "requires a reachable PXE via AZTEC_PXE_URL"]
async fn pxe_register_account_roundtrip() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };
    let secret_key = Fr::from(0xdead_cafe_u64);
    let partial_address = Fr::from(0xbeef_u64);

    let complete = pxe
        .register_account(&secret_key, &partial_address)
        .await
        .expect("register account");

    assert_eq!(complete.partial_address, partial_address);
    assert_ne!(complete.address, AztecAddress(Fr::zero()));

    // Re-registering the same key should be idempotent.
    let again = pxe
        .register_account(&secret_key, &partial_address)
        .await
        .expect("re-register account");
    assert_eq!(again.address, complete.address);

    // The account should appear in the registered list.
    let accounts = pxe
        .get_registered_accounts()
        .await
        .expect("get registered accounts");
    assert!(
        accounts.iter().any(|a| a.address == complete.address),
        "newly registered account should be in the list"
    );
}

// ---------------------------------------------------------------------------
// Senders
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a reachable PXE via AZTEC_PXE_URL"]
async fn pxe_sender_lifecycle() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };
    let sender = AztecAddress(Fr::from(0x1234_5678_u64));

    // Register
    let registered = pxe.register_sender(&sender).await.expect("register sender");
    assert_eq!(registered, sender);

    // Should appear in list
    let senders = pxe.get_senders().await.expect("get senders");
    assert!(
        senders.contains(&sender),
        "registered sender should be in list"
    );

    // Remove
    pxe.remove_sender(&sender).await.expect("remove sender");

    // Should no longer appear
    let senders = pxe.get_senders().await.expect("get senders after remove");
    assert!(
        !senders.contains(&sender),
        "removed sender should not be in list"
    );
}

// ---------------------------------------------------------------------------
// Contracts
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a reachable PXE via AZTEC_PXE_URL"]
async fn pxe_get_contracts() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };
    let contracts = pxe.get_contracts().await.expect("get contracts");
    eprintln!("registered contracts: {}", contracts.len());
}

#[tokio::test]
#[ignore = "requires a reachable PXE via AZTEC_PXE_URL"]
async fn pxe_get_unknown_contract_instance_returns_none() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };
    let unknown = AztecAddress(Fr::from(0xdead_dead_u64));
    let result = pxe
        .get_contract_instance(&unknown)
        .await
        .expect("get contract instance for unknown address");
    assert!(result.is_none(), "unknown address should return None");
}

#[tokio::test]
#[ignore = "requires a reachable PXE via AZTEC_PXE_URL"]
async fn pxe_get_unknown_contract_artifact_returns_none() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };
    let unknown_class_id = Fr::from(0xbad_c1a55_u64);
    let result = pxe
        .get_contract_artifact(&unknown_class_id)
        .await
        .expect("get contract artifact for unknown class");
    assert!(result.is_none(), "unknown class ID should return None");
}

// ---------------------------------------------------------------------------
// Contract registration with fixture
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a reachable PXE via AZTEC_PXE_URL"]
async fn pxe_register_contract_class_from_fixture() {
    use aztec_rs::abi::ContractArtifact;

    let Some(pxe) = require_live_pxe().await else {
        return;
    };

    let json = include_str!("../fixtures/counter_contract.json");
    let artifact = ContractArtifact::from_json(json).expect("parse counter_contract.json");

    // Registering a contract class should succeed (or be idempotent).
    pxe.register_contract_class(&artifact)
        .await
        .expect("register contract class");
}

// ---------------------------------------------------------------------------
// Wire-format smoke tests
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires a reachable PXE via AZTEC_PXE_URL"]
async fn pxe_block_header_deserializes_cleanly() {
    let Some(pxe) = require_live_pxe().await else {
        return;
    };
    let header = pxe
        .get_synced_block_header()
        .await
        .expect("get synced block header");
    // Verify we can round-trip through serde without losing data.
    let serialized = serde_json::to_string(&header).expect("serialize header");
    let _: aztec_rs::pxe::BlockHeader =
        serde_json::from_str(&serialized).expect("deserialize header");
}
