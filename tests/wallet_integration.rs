//! Integration tests for `BaseWallet` against a live Aztec PXE + node.
//!
//! These tests are `#[ignore]`d by default because they require a running
//! Aztec network with both PXE and node endpoints. Run them with:
//!
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 \
//! AZTEC_PXE_URL=http://localhost:8080 \
//! cargo test --test wallet_integration -- --ignored
//! ```

#![allow(clippy::expect_used, clippy::print_stderr)]

use async_trait::async_trait;

use aztec_rs::abi::ContractArtifact;
use aztec_rs::fee::GasSettings;
use aztec_rs::node::{create_aztec_node_client, AztecNode as _};
use aztec_rs::pxe::{create_pxe_client, Pxe as _, TxExecutionRequest};
use aztec_rs::tx::{AuthWitness, ExecutionPayload};
use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
use aztec_rs::wallet::{
    AccountProvider, Aliased, BaseWallet, ChainInfo, MessageHashOrIntent, Wallet,
};
use aztec_rs::Error;

// ---------------------------------------------------------------------------
// Minimal no-op AccountProvider for read-only wallet tests
// ---------------------------------------------------------------------------

struct EmptyAccountProvider;

#[async_trait]
impl AccountProvider for EmptyAccountProvider {
    async fn create_tx_execution_request(
        &self,
        from: &AztecAddress,
        _exec: ExecutionPayload,
        _gas_settings: GasSettings,
        _chain_info: &ChainInfo,
        _fee_payer: Option<AztecAddress>,
    ) -> Result<TxExecutionRequest, Error> {
        Err(Error::InvalidData(format!(
            "EmptyAccountProvider cannot create tx for {from}"
        )))
    }

    async fn create_auth_wit(
        &self,
        from: &AztecAddress,
        _intent: MessageHashOrIntent,
        _chain_info: &ChainInfo,
    ) -> Result<AuthWitness, Error> {
        Err(Error::InvalidData(format!(
            "EmptyAccountProvider cannot create auth wit for {from}"
        )))
    }

    async fn get_complete_address(
        &self,
        _address: &AztecAddress,
    ) -> Result<Option<CompleteAddress>, Error> {
        Ok(None)
    }

    async fn get_accounts(&self) -> Result<Vec<Aliased<AztecAddress>>, Error> {
        Ok(vec![])
    }
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

async fn require_live_wallet() -> Option<
    BaseWallet<aztec_rs::pxe::HttpPxeClient, aztec_rs::node::HttpNodeClient, EmptyAccountProvider>,
> {
    let node_url =
        std::env::var("AZTEC_NODE_URL").unwrap_or_else(|_| "http://localhost:8080".to_owned());
    let pxe_url =
        std::env::var("AZTEC_PXE_URL").unwrap_or_else(|_| "http://localhost:8080".to_owned());

    let node = create_aztec_node_client(&node_url);
    let pxe = create_pxe_client(&pxe_url);

    // Single attempt for each — fail fast instead of retrying for 120s per test.
    if let Err(err) = node.get_node_info().await {
        eprintln!("skipping: node not reachable: {err}");
        return None;
    }
    if let Err(err) = pxe.get_synced_block_header().await {
        eprintln!("skipping: PXE not reachable: {err}");
        return None;
    }

    Some(BaseWallet::new(pxe, node, EmptyAccountProvider))
}

// ---------------------------------------------------------------------------
// Chain info
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live PXE + node via AZTEC_PXE_URL and AZTEC_NODE_URL"]
async fn wallet_get_chain_info() {
    let Some(wallet) = require_live_wallet().await else {
        return;
    };
    let info = wallet.get_chain_info().await.expect("get chain info");
    assert_ne!(info.chain_id, Fr::zero(), "chain ID should be non-zero");
    assert_ne!(info.version, Fr::zero(), "version should be non-zero");
    eprintln!("chain_id={}, version={}", info.chain_id, info.version);
}

// ---------------------------------------------------------------------------
// Accounts (empty provider)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live PXE + node via AZTEC_PXE_URL and AZTEC_NODE_URL"]
async fn wallet_get_accounts_empty_provider() {
    let Some(wallet) = require_live_wallet().await else {
        return;
    };
    let accounts = wallet.get_accounts().await.expect("get accounts");
    assert!(
        accounts.is_empty(),
        "empty provider should return no accounts"
    );
}

// ---------------------------------------------------------------------------
// Address book (senders)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live PXE + node via AZTEC_PXE_URL and AZTEC_NODE_URL"]
async fn wallet_sender_roundtrip() {
    let Some(wallet) = require_live_wallet().await else {
        return;
    };
    let addr = AztecAddress(Fr::from(0xaabb_ccdd_u64));

    // Register via wallet
    let result = wallet
        .register_sender(addr, Some("integration-test".into()))
        .await
        .expect("register sender");
    assert_eq!(result, addr);

    // Should appear in address book
    let book = wallet.get_address_book().await.expect("get address book");
    assert!(
        book.iter().any(|entry| entry.item == addr),
        "registered sender should appear in address book"
    );
}

// ---------------------------------------------------------------------------
// Contract metadata for unknown address
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live PXE + node via AZTEC_PXE_URL and AZTEC_NODE_URL"]
async fn wallet_contract_metadata_unknown() {
    let Some(wallet) = require_live_wallet().await else {
        return;
    };
    let unknown = AztecAddress(Fr::from(0xdead_beef_u64));
    let meta = wallet
        .get_contract_metadata(unknown)
        .await
        .expect("get contract metadata for unknown");
    assert!(meta.instance.is_none());
    assert!(!meta.is_contract_published);
    assert!(!meta.is_contract_initialized);
    assert!(!meta.is_contract_updated);
}

// ---------------------------------------------------------------------------
// Contract class metadata for unknown class
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live PXE + node via AZTEC_PXE_URL and AZTEC_NODE_URL"]
async fn wallet_contract_class_metadata_unknown() {
    let Some(wallet) = require_live_wallet().await else {
        return;
    };
    let meta = wallet
        .get_contract_class_metadata(Fr::from(0xbad_c1a55_u64))
        .await
        .expect("get contract class metadata for unknown");
    assert!(!meta.is_artifact_registered);
    assert!(!meta.is_contract_class_publicly_registered);
}

// ---------------------------------------------------------------------------
// Contract registration via wallet
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live PXE + node via AZTEC_PXE_URL and AZTEC_NODE_URL"]
async fn wallet_register_contract_with_fixture() {
    use aztec_rs::pxe::Pxe;
    use aztec_rs::types::{ContractInstance, ContractInstanceWithAddress, PublicKeys};

    let Some(wallet) = require_live_wallet().await else {
        return;
    };

    let json = include_str!("../fixtures/counter_contract.json");
    let artifact = ContractArtifact::from_json(json).expect("parse counter_contract.json");

    // Register the class first so the artifact is available.
    wallet
        .pxe()
        .register_contract_class(&artifact)
        .await
        .expect("register contract class");

    // Build a synthetic instance for registration.
    let instance = ContractInstanceWithAddress {
        address: AztecAddress(Fr::from(0x10e6_u64)),
        inner: ContractInstance {
            version: 1,
            salt: Fr::from(1u64),
            deployer: AztecAddress(Fr::zero()),
            current_contract_class_id: Fr::from(0xc1a55_u64),
            original_contract_class_id: Fr::from(0xc1a55_u64),
            initialization_hash: Fr::zero(),
            public_keys: PublicKeys::default(),
        },
    };

    let result = wallet
        .register_contract(instance.clone(), Some(artifact), None)
        .await
        .expect("register contract via wallet");
    assert_eq!(result.address, instance.address);

    // Metadata should now show the instance (not published, since it's local).
    let meta = wallet
        .get_contract_metadata(instance.address)
        .await
        .expect("get metadata after registration");
    assert!(meta.instance.is_some());
    assert!(!meta.is_contract_published);
}

// ---------------------------------------------------------------------------
// End-to-end: chain info drives consistent wallet state
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live PXE + node via AZTEC_PXE_URL and AZTEC_NODE_URL"]
async fn wallet_chain_info_matches_node() {
    use aztec_rs::node::AztecNode;

    let Some(wallet) = require_live_wallet().await else {
        return;
    };

    let chain_info = wallet.get_chain_info().await.expect("wallet chain info");
    let node_info = wallet.node().get_node_info().await.expect("node info");

    assert_eq!(chain_info.chain_id, Fr::from(node_info.l1_chain_id));
    assert_eq!(chain_info.version, Fr::from(node_info.rollup_version));
}
