//! Shared utilities for e2e tests.
//!
//! Import in any integration test via:
//! ```rust,ignore
//! mod common;
//! use common::*;
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::cast_possible_wrap,
    dead_code,
    unused_imports
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub use aztec_rs::abi::ContractArtifact;
pub use aztec_rs::account::{SchnorrAccountContract, SingleAccountProvider};
pub use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
pub use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
pub use aztec_rs::node::{create_aztec_node_client, HttpNodeClient};
pub use aztec_rs::types::{AztecAddress, CompleteAddress, Fr};
pub use aztec_rs::wallet::BaseWallet;

// ---------------------------------------------------------------------------
// Type aliases
// ---------------------------------------------------------------------------

/// The standard wallet type used by most e2e tests.
pub type TestWallet = BaseWallet<EmbeddedPxe<HttpNodeClient>, HttpNodeClient, SingleAccountProvider>;

/// Arc-wrapped wallet variant used by tests that need shared ownership.
pub type SharedTestWallet = Arc<TestWallet>;

// ---------------------------------------------------------------------------
// Test accounts (mirrors upstream fixtures/fixtures.ts)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ImportedTestAccount {
    pub alias: &'static str,
    pub address: &'static str,
    pub secret_key: &'static str,
    pub partial_address: &'static str,
}

pub const TEST_ACCOUNT_0: ImportedTestAccount = ImportedTestAccount {
    alias: "test0",
    address: "0x0a60414ee907527880b7a53d4dacdeb9ef768bb98d9d8d1e7200725c13763331",
    secret_key: "0x2153536ff6628eee01cf4024889ff977a18d9fa61d0e414422f7681cf085c281",
    partial_address: "0x140c3a658e105092549c8402f0647fe61d87aba4422b484dfac5d4a87462eeef",
};

pub const TEST_ACCOUNT_1: ImportedTestAccount = ImportedTestAccount {
    alias: "test1",
    address: "0x00cedf87a800bd88274762d77ffd93e97bc881d1fc99570d62ba97953597914d",
    secret_key: "0x0aebd1b4be76efa44f5ee655c20bf9ea60f7ae44b9a7fd1fd9f189c7a0b0cdae",
    partial_address: "0x0325ee1689daec508c6adef0df4a1e270ac1fcf971fed1f893b2d98ad12d6bb8",
};

pub const TEST_ACCOUNT_2: ImportedTestAccount = ImportedTestAccount {
    alias: "test2",
    address: "0x1dd551228da3a56b5da5f5d73728e08d8114f59897c27136f1bcdd4c05028905",
    secret_key: "0x0f6addf0da06c33293df974a565b03d1ab096090d907d98055a8b7f4954e120c",
    partial_address: "0x17604ccd69bd09d8df02c4a345bc4232e5d24b568536c55407b3e4e4e3354c4c",
};

// ---------------------------------------------------------------------------
// Environment helpers
// ---------------------------------------------------------------------------

/// Returns the Aztec node URL from the `AZTEC_NODE_URL` env var,
/// defaulting to `http://localhost:8080`.
pub fn node_url() -> String {
    std::env::var("AZTEC_NODE_URL").unwrap_or_else(|_| "http://localhost:8080".to_owned())
}

/// Returns the repository root (the directory containing `Cargo.toml`).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

// ---------------------------------------------------------------------------
// Concurrency / uniqueness helpers
// ---------------------------------------------------------------------------

/// Acquires a process-wide mutex to serialize tests that cannot run concurrently.
pub fn serial_guard() -> MutexGuard<'static, ()> {
    static E2E_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    E2E_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Returns a unique salt seeded from the current timestamp, incrementing on
/// each call. Useful for deploying multiple contract instances in a single test
/// run without collision.
#[allow(clippy::cast_possible_truncation)]
pub fn next_unique_salt() -> u64 {
    static NEXT_SALT: OnceLock<AtomicU64> = OnceLock::new();
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    NEXT_SALT
        .get_or_init(|| AtomicU64::new(seed))
        .fetch_add(1, Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Account helpers
// ---------------------------------------------------------------------------

/// Derives a [`CompleteAddress`] from an [`ImportedTestAccount`] and asserts
/// that the derived address matches the expected one in the fixture.
pub fn imported_complete_address(account: ImportedTestAccount) -> CompleteAddress {
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

// ---------------------------------------------------------------------------
// Artifact loaders
// ---------------------------------------------------------------------------

/// Tries each candidate path in order and returns the first artifact that
/// parses successfully. Panics if none are found.
pub fn load_artifact_from_candidates(
    display_name: &str,
    candidates: &[PathBuf],
) -> ContractArtifact {
    for path in candidates {
        if let Ok(json) = fs::read_to_string(path) {
            return ContractArtifact::from_nargo_json(&json)
                .unwrap_or_else(|e| panic!("parse {display_name} from {}: {e}", path.display()));
        }
    }

    let searched = candidates
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    panic!("could not locate {display_name}; searched: {searched}");
}

/// Tries each candidate path in order and returns the first artifact that
/// parses successfully, or `None` if none are found.
pub fn try_load_artifact_from_candidates(candidates: &[PathBuf]) -> Option<ContractArtifact> {
    for path in candidates {
        if let Ok(json) = fs::read_to_string(path) {
            return ContractArtifact::from_nargo_json(&json).ok();
        }
    }
    None
}

pub fn load_token_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/token_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse token_contract_compiled.json")
}

pub fn load_schnorr_account_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/schnorr_account_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse schnorr_account_contract_compiled.json")
}

pub fn load_child_contract_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/child_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse child_contract_compiled.json")
}

pub fn load_parent_contract_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/parent_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse parent_contract_compiled.json")
}

pub fn load_generic_proxy_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/generic_proxy_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse generic_proxy_contract_compiled.json")
}

pub fn load_test_contract_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/test_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse test_contract_compiled.json")
}

pub fn load_stateful_test_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/stateful_test_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse stateful_test_contract_compiled.json")
}

pub fn load_pending_note_hashes_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/pending_note_hashes_contract_compiled.json");
    ContractArtifact::from_nargo_json(json)
        .expect("parse pending_note_hashes_contract_compiled.json")
}

pub fn load_invalid_account_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/invalid_account_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse invalid_account_contract_compiled.json")
}

pub fn load_state_vars_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/state_vars_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse state_vars_contract_compiled.json")
}

pub fn load_scope_test_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/scope_test_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse scope_test_contract_compiled.json")
}

pub fn load_note_getter_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/note_getter_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse note_getter_contract_compiled.json")
}

pub fn load_static_parent_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/static_parent_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse static_parent_contract_compiled.json")
}

pub fn load_static_child_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/static_child_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse static_child_contract_compiled.json")
}

pub fn load_test_log_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/test_log_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse test_log_contract_compiled.json")
}

pub fn load_auth_contract_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/auth_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse auth_contract_compiled.json")
}

pub fn load_auth_wit_test_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/auth_wit_test_contract_compiled.json");
    ContractArtifact::from_nargo_json(json).expect("parse auth_wit_test_contract_compiled.json")
}

pub fn load_escrow_artifact() -> ContractArtifact {
    let json = include_str!("../../fixtures/escrow_contract.json");
    ContractArtifact::from_nargo_json(json).expect("parse escrow_contract.json")
}

/// Loads the FPC artifact if available (from fixtures or upstream).
pub fn load_fpc_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/fpc_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/fpc_contract-FPC.json"),
    ])
}

/// Loads the no-constructor artifact if available (from fixtures or upstream).
pub fn load_no_constructor_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/no_constructor_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/no_constructor_contract-NoConstructor.json"),
    ])
}

// ---------------------------------------------------------------------------
// Common error string constants
// ---------------------------------------------------------------------------

pub const U128_UNDERFLOW_ERROR: &str = "attempt to subtract with overflow";
pub const U128_OVERFLOW_ERROR: &str = "attempt to add with overflow";
