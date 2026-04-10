//! Token minting tests — 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/minting.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_minting -- --ignored --nocapture
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
// Tests: e2e_token_contract minting — Public
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_mint_as_minter() {
    // TODO: deploy token, set account_0 as minter, mint public tokens, verify balance
    todo!("mirror upstream: Public > as minter")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_mint_as_non_minter() {
    // TODO: attempt public mint from non-minter account, expect revert
    todo!("mirror upstream: Public > failure cases > as non-minter")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_mint_recipient_balance_overflow() {
    // TODO: mint amount < u128 but recipient balance would exceed u128, expect revert
    todo!("mirror upstream: Public > failure cases > mint <u128 but recipient balance >u128")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn public_mint_total_supply_overflow() {
    // TODO: mint amount < u128 but total supply would exceed u128, expect revert
    todo!("mirror upstream: Public > failure cases > mint <u128 but such that total supply >u128")
}

// ---------------------------------------------------------------------------
// Tests: e2e_token_contract minting — Private
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mint_as_minter() {
    // TODO: deploy token, set account_0 as minter, mint private tokens, verify balance
    todo!("mirror upstream: Private > as minter")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mint_as_non_minter() {
    // TODO: attempt private mint from non-minter account, expect revert
    todo!("mirror upstream: Private > failure cases > as non-minter")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mint_overflow() {
    // TODO: attempt to mint > u128 tokens, expect revert
    todo!("mirror upstream: Private > failure cases > mint >u128 tokens to overflow")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mint_recipient_balance_overflow() {
    // TODO: mint amount < u128 but recipient balance would exceed u128, expect revert
    todo!("mirror upstream: Private > failure cases > mint <u128 but recipient balance >u128")
}

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn private_mint_total_supply_overflow() {
    // TODO: mint amount < u128 but total supply would exceed u128, expect revert
    todo!("mirror upstream: Private > failure cases > mint <u128 but such that total supply >u128")
}
