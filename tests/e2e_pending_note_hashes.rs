//! Pending note hashes tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_pending_note_hashes_contract.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_pending_note_hashes -- --ignored --nocapture
//! ```

#![allow(
    clippy::todo,
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

// TODO: need a pending_note_hashes test contract artifact
// fn load_pending_note_hashes_artifact() -> ContractArtifact {
//     let json = include_str!("../fixtures/pending_note_hashes_contract_compiled.json");
//     ContractArtifact::from_nargo_json(json).expect("parse pending_note_hashes_contract_compiled.json")
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
// Tests: e2e_pending_note_hashes_contract
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn function_can_get_notes_it_just_inserted() {
    // TODO: deploy contract, insert note then immediately get it in same TX
    todo!("mirror upstream: Aztec.nr function can 'get' notes it just 'inserted'")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn squash_create_and_nullify_in_same_tx() {
    // TODO: create and nullify a note in the same TX, verify squashing
    todo!(
        "mirror upstream: Squash! Aztec.nr function can 'create' and 'nullify' note in the same TX"
    )
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn squash_create_and_nullify_in_same_tx_with_2_note_logs() {
    // TODO: same as above but with 2 note logs emitted
    todo!("mirror upstream: Squash! ... with 2 note logs")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn squash_create_2_notes_and_nullify_both_in_same_tx() {
    // TODO: create 2 notes and nullify both in same TX, verify both squashed
    todo!("mirror upstream: Squash! ... create 2 notes and nullify both in the same TX")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn squash_create_2_notes_and_nullify_1_in_same_tx() {
    // TODO: create 2 notes, nullify 1, kernel squashes one note+nullifier pair
    todo!("mirror upstream: Squash! ... create 2 notes and nullify 1 in the same TX")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn squash_create_2_notes_same_hash_nullify_1_in_same_tx() {
    // TODO: create 2 notes with identical hash, nullify 1, verify correct squashing
    todo!("mirror upstream: Squash! ... create 2 notes with the same note hash and nullify 1 in the same TX")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn squash_nullify_pending_and_persistent_in_same_tx() {
    // TODO: create a persistent note in TX1, then in TX2 create a pending note
    //       and nullify both the pending and persistent note
    todo!("mirror upstream: Squash! ... nullify a pending note and a persistent in the same TX")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn get_notes_filters_nullified_note_from_previous_tx() {
    // TODO: create note in TX1, nullify in TX2, get_notes in TX3 should not return it
    todo!("mirror upstream: get_notes function filters a nullified note created in a previous transaction")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn handle_overflowing_kernel_data_in_nested_calls() {
    // TODO: nested calls that produce enough side effects to overflow kernel data structures
    todo!("mirror upstream: Should handle overflowing the kernel data structures in nested calls")
}
