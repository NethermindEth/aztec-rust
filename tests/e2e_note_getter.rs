//! Note getter tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_note_getter.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_note_getter -- --ignored --nocapture
//! ```

#![allow(
    clippy::expect_used,
    clippy::print_stderr,
    clippy::similar_names,
    dead_code,
    unused_imports
)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionType};
use aztec_rs::account::{SchnorrAccountContract, SingleAccountProvider};
use aztec_rs::contract::Contract;
use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
use aztec_rs::deployment::DeployOptions;
use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
use aztec_rs::pxe::{Pxe, RegisterContractRequest};
use aztec_rs::tx::FunctionCall;
use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
use aztec_rs::wallet::{BaseWallet, SendOptions, Wallet};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

// TODO: need a note_getter test contract artifact (DocsExample or similar)
// fn load_note_getter_test_artifact() -> ContractArtifact {
//     let json = include_str!("../fixtures/docs_example_contract_compiled.json");
//     ContractArtifact::from_nargo_json(json).expect("parse docs_example_contract_compiled.json")
// }

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

// ---------------------------------------------------------------------------
// Tests: e2e_note_getter — comparators
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn inserts_notes_then_queries_with_all_comparators() {
    // TODO: deploy DocsExample contract, insert notes with values 0-9,
    //       query with EQ, NEQ, LT, GT, LTE, GTE comparators, verify results
    todo!("mirror upstream: inserts notes from 0-9, then makes multiple queries specifying the total suite of comparators")
}

// ---------------------------------------------------------------------------
// Tests: e2e_note_getter — status filter — active note only
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn active_status_returns_active_notes() {
    // TODO: create notes, query with ACTIVE status, verify only active notes returned
    todo!("mirror upstream: active note only > returns active notes")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn active_status_does_not_return_nullified_notes() {
    // TODO: create notes, nullify some, query with ACTIVE status,
    //       verify nullified notes excluded
    todo!("mirror upstream: active note only > does not return nullified notes")
}

// ---------------------------------------------------------------------------
// Tests: e2e_note_getter — status filter — active and nullified notes
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn active_and_nullified_returns_active_notes() {
    // TODO: create notes, query with ACTIVE_AND_NULLIFIED status,
    //       verify active notes included
    todo!("mirror upstream: active and nullified notes > returns active notes")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn active_and_nullified_returns_nullified_notes() {
    // TODO: create notes, nullify some, query with ACTIVE_AND_NULLIFIED status,
    //       verify nullified notes included
    todo!("mirror upstream: active and nullified notes > returns nullified notes")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn active_and_nullified_returns_both() {
    // TODO: create notes, nullify some, query with ACTIVE_AND_NULLIFIED status,
    //       verify both active and nullified notes returned
    todo!("mirror upstream: active and nullified notes > returns both active and nullified notes")
}
