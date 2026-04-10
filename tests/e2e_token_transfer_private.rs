//! Token private transfer tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/transfer_in_private.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_transfer_private -- --ignored --nocapture
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

fn load_compiled_token_artifact() -> ContractArtifact {
    let json = include_str!("../fixtures/token_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse token_contract_compiled.json")
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

// ---------------------------------------------------------------------------
// Tests: e2e_token_contract transfer private
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other() {
    // TODO: deploy token, mint to account_0, create authwit for account_1 to
    //       transfer on behalf of account_0, verify balances
    todo!("mirror upstream: transfer on behalf of other")
}

// -- failure cases --

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_self_with_non_zero_nonce() {
    // TODO: attempt private transfer on behalf of self with nonce != 0, expect revert
    todo!("mirror upstream: transfer on behalf of self with non-zero nonce")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_more_than_balance_on_behalf_of_other() {
    // TODO: create authwit, attempt transfer exceeding balance, expect revert
    todo!("mirror upstream: transfer more than balance on behalf of other")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_without_approval() {
    // TODO: attempt transfer without authwit, expect revert
    todo!("mirror upstream: transfer on behalf of other without approval")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_wrong_designated_caller() {
    // TODO: create authwit for wrong caller, attempt transfer, expect revert
    todo!("mirror upstream: transfer on behalf of other, wrong designated caller")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_cancelled_authwit() {
    // TODO: create authwit, cancel it, attempt transfer, expect revert
    todo!("mirror upstream: transfer on behalf of other, cancelled authwit")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_on_behalf_of_other_invalid_verify_private_authwit() {
    // TODO: create invalid authwit on "from", attempt transfer, expect revert
    todo!("mirror upstream: transfer on behalf of other, invalid verify_private_authwit on from")
}
