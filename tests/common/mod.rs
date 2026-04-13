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

use std::collections::BTreeMap;

pub use aztec_rs::abi::{AbiValue, ContractArtifact, FunctionSelector, FunctionType};
pub use aztec_rs::account::{AccountContract, SchnorrAccountContract, SingleAccountProvider};
pub use aztec_rs::contract::Contract;
pub use aztec_rs::crypto::complete_address_from_secret_key_and_partial_address;
pub use aztec_rs::deployment::DeployOptions;
pub use aztec_rs::embedded_pxe::stores::note_store::StoredNote;
pub use aztec_rs::embedded_pxe::{EmbeddedPxe, InMemoryKvStore};
pub use aztec_rs::hash::{
    compute_contract_address_from_instance, compute_contract_class_id_from_artifact,
};
pub use aztec_rs::node::{create_aztec_node_client, AztecNode, HttpNodeClient};
pub use aztec_rs::pxe::{Pxe, RegisterContractRequest};
pub use aztec_rs::tx::{ExecutionPayload, FunctionCall};
pub use aztec_rs::types::{
    AztecAddress, CompleteAddress, ContractInstance, ContractInstanceWithAddress, Fr, PublicKeys,
};
pub use aztec_rs::wallet::{
    BaseWallet, ExecuteUtilityOptions, SendOptions, SimulateOptions, Wallet,
};

pub use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Type aliases
// ---------------------------------------------------------------------------

/// The standard wallet type used by most e2e tests.
pub type TestWallet =
    BaseWallet<EmbeddedPxe<HttpNodeClient>, HttpNodeClient, SingleAccountProvider>;

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

/// Loads the escrow contract's compiled artifact (with bytecode) if available,
/// either from local fixtures or from the upstream noir-contracts target dir.
pub fn load_escrow_compiled_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/escrow_contract_compiled.json"),
        root.join(
            "../aztec-packages/noir-projects/noir-contracts/target/escrow_contract-Escrow.json",
        ),
    ])
}

/// Loads the NFT contract compiled artifact if available.
pub fn load_nft_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/nft_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/nft_contract-NFT.json"),
    ])
}

/// Loads the event_only contract compiled artifact if available.
pub fn load_event_only_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/event_only_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/event_only_contract-EventOnly.json"),
    ])
}

/// Loads the abi_types test contract artifact if available.
pub fn load_abi_types_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/abi_types_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/abi_types_contract-AbiTypes.json"),
    ])
}

/// Loads the option_param test contract artifact if available.
pub fn load_option_param_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/option_param_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/option_param_contract-OptionParam.json"),
    ])
}

/// Loads the import_test contract artifact if available.
pub fn load_import_test_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/import_test_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/import_test_contract-ImportTest.json"),
    ])
}

/// Loads the AMM contract artifact if available.
pub fn load_amm_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/amm_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/amm_contract-AMM.json"),
    ])
}

/// Loads the sponsored FPC (no-end-setup variant) test contract if available.
pub fn load_sponsored_fpc_no_end_setup_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/sponsored_fpc_no_end_setup_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/sponsored_fpc_no_end_setup_contract-SponsoredFPCNoEndSetup.json"),
    ])
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

pub fn load_sponsored_fpc_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/sponsored_fpc_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/sponsored_fpc_contract-SponsoredFPC.json"),
    ])
}

pub fn load_offchain_effect_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/offchain_effect_contract_compiled.json")
    ])
}

pub fn load_updatable_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[root.join("fixtures/updatable_contract_compiled.json")])
}

pub fn load_updated_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[root.join("fixtures/updated_contract_compiled.json")])
}

// ---------------------------------------------------------------------------
// Common error string constants
// ---------------------------------------------------------------------------

pub const U128_UNDERFLOW_ERROR: &str = "attempt to subtract with overflow";
pub const U128_OVERFLOW_ERROR: &str = "attempt to add with overflow";

// ---------------------------------------------------------------------------
// Signing key note helpers
// ---------------------------------------------------------------------------

/// Build a [`StoredNote`] representing a Schnorr signing key for `owner`.
///
/// Pre-imported accounts' signing key notes are not discoverable on-chain, so
/// we inject a synthetic note into the PXE's note store.  The `nullifier_seed`
/// must be unique per note to avoid duplicate-nullifier errors when multiple
/// notes are injected in the same test.
pub fn make_signing_key_note(
    account_contract: &SchnorrAccountContract,
    owner: AztecAddress,
    nullifier_seed: u64,
) -> StoredNote {
    let signing_pk = account_contract.signing_public_key();
    // Construct a deterministic but unique siloed nullifier from the seed.
    let mut hex = format!("0xdeadbeef{nullifier_seed:0>56x}");
    // Ensure the hex string is exactly 66 chars (0x + 64 hex digits).
    hex.truncate(66);
    StoredNote {
        contract_address: owner,
        owner,
        storage_slot: Fr::from(1u64),
        randomness: Fr::zero(),
        note_nonce: Fr::from(1u64),
        note_hash: Fr::from(1u64),
        siloed_nullifier: Fr::from_hex(&hex).expect("unique nullifier"),
        note_data: vec![signing_pk.x, signing_pk.y],
        nullified: false,
        is_pending: false,
        nullification_block_number: None,
        leaf_index: None,
        block_number: None,
        tx_index_in_block: None,
        note_index_in_tx: None,
        scopes: vec![owner],
    }
}

/// Inject a Schnorr signing key note into a PXE's note store.
pub async fn seed_signing_key_note(
    pxe: &EmbeddedPxe<HttpNodeClient>,
    account_contract: &SchnorrAccountContract,
    owner: AztecAddress,
    nullifier_seed: u64,
) {
    let note = make_signing_key_note(account_contract, owner, nullifier_seed);
    pxe.note_store()
        .add_note(&note)
        .await
        .expect("seed signing key note");
}

// ---------------------------------------------------------------------------
// Wallet setup (registers account contract + signing key note in PXE)
// ---------------------------------------------------------------------------

/// Creates a wallet for a pre-imported test account.
///
/// Registers the Schnorr account contract artifact, instance, and signing key
/// note in the PXE so that `execute_entrypoint_via_acvm` can run the real Noir
/// entrypoint circuit (required for public function calls).
pub async fn setup_wallet(account: ImportedTestAccount) -> Option<(TestWallet, AztecAddress)> {
    let url = node_url();
    let node = create_aztec_node_client(&url);
    if node.get_node_info().await.is_err() {
        return None;
    }

    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = EmbeddedPxe::create(node.clone(), kv).await.ok()?;

    let secret_key = Fr::from_hex(account.secret_key).expect("valid sk");
    let complete = imported_complete_address(account);

    pxe.key_store().add_account(&secret_key).await.ok()?;
    pxe.address_store().add(&complete).await.ok()?;

    let account_contract = SchnorrAccountContract::new(secret_key);

    // Register account contract artifact + instance in PXE
    let compiled_account_artifact = load_schnorr_account_artifact();
    let dynamic_artifact = account_contract.contract_artifact().await.ok()?;
    let dynamic_class_id = compute_contract_class_id_from_artifact(&dynamic_artifact).ok()?;

    pxe.contract_store()
        .add_artifact(&dynamic_class_id, &compiled_account_artifact)
        .await
        .ok()?;
    let account_instance = ContractInstanceWithAddress {
        address: complete.address,
        inner: ContractInstance {
            version: 1,
            salt: Fr::from(0u64),
            deployer: AztecAddress::zero(),
            current_contract_class_id: dynamic_class_id,
            original_contract_class_id: dynamic_class_id,
            initialization_hash: Fr::zero(),
            public_keys: complete.public_keys.clone(),
        },
    };
    pxe.contract_store()
        .add_instance(&account_instance)
        .await
        .ok()?;

    seed_signing_key_note(&pxe, &account_contract, complete.address, 1).await;

    // Register protocol contracts so the ACVM can execute them
    register_protocol_contracts(&pxe).await;

    let provider =
        SingleAccountProvider::new(complete.clone(), Box::new(account_contract), account.alias);
    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, complete.address))
}

/// Register protocol contract artifacts in the PXE.
///
/// Mirrors upstream `PXE.registerProtocolContracts()`. Required so the ACVM
/// can execute protocol contract functions (e.g., FeeJuice.claim).
pub async fn register_protocol_contracts(pxe: &EmbeddedPxe<HttpNodeClient>) {
    // FeeJuice protocol contract (address 0x05)
    if let Some(artifact) = load_fee_juice_artifact() {
        let fee_juice_address = aztec_rs::constants::protocol_contract_address::fee_juice();
        let class_id = compute_contract_class_id_from_artifact(&artifact).unwrap_or(Fr::zero());
        let _ = pxe
            .contract_store()
            .add_artifact(&class_id, &artifact)
            .await;
        let instance = ContractInstanceWithAddress {
            address: fee_juice_address,
            inner: ContractInstance {
                version: 1,
                salt: Fr::zero(),
                deployer: AztecAddress::zero(),
                current_contract_class_id: class_id,
                original_contract_class_id: class_id,
                initialization_hash: Fr::zero(),
                public_keys: PublicKeys::default(),
            },
        };
        let _ = pxe.contract_store().add_instance(&instance).await;
    }
}

pub fn load_fee_juice_artifact() -> Option<ContractArtifact> {
    let root = repo_root();
    try_load_artifact_from_candidates(&[
        root.join("fixtures/fee_juice_contract_compiled.json"),
        root.join("../aztec-packages/noir-projects/noir-contracts/target/fee_juice_contract-FeeJuice.json"),
    ])
}

// ---------------------------------------------------------------------------
// Public storage helpers
// ---------------------------------------------------------------------------

/// Derive a storage slot for a key in a Map storage variable.
pub fn derive_storage_slot_in_map(base_slot: u64, key: &AztecAddress) -> Fr {
    const DOM_SEP_PUBLIC_STORAGE_MAP_SLOT: u32 = 4_015_149_901;
    aztec_rs::hash::poseidon2_hash_with_separator(
        &[Fr::from(base_slot), Fr::from(*key)],
        DOM_SEP_PUBLIC_STORAGE_MAP_SLOT,
    )
}

// ---------------------------------------------------------------------------
// Wallet setup with extra accounts (authwit-capable)
// ---------------------------------------------------------------------------

/// Registers an account's Schnorr contract artifact and instance on the PXE so
/// that auth-witness inner-hash lookups resolve correctly.
pub async fn register_account_for_authwit(
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

    let class_id =
        compute_contract_class_id_from_artifact(&dynamic_artifact).expect("compute class id");
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

/// Creates a wallet for `primary` with `extra` accounts also registered,
/// including authwit contract registration and signing key note injection.
/// This is the full-featured variant used by token tests that need authwit.
pub async fn create_wallet(
    primary: ImportedTestAccount,
    extra: &[ImportedTestAccount],
) -> Option<(SharedTestWallet, AztecAddress)> {
    let url = node_url();
    let node = create_aztec_node_client(&url);
    if node.get_node_info().await.is_err() {
        return None;
    }

    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = EmbeddedPxe::create(node.clone(), kv).await.ok()?;

    let secret_key = Fr::from_hex(primary.secret_key).expect("valid primary secret key");
    let complete = imported_complete_address(primary);
    pxe.key_store()
        .add_account(&secret_key)
        .await
        .expect("seed key store");
    pxe.address_store()
        .add(&complete)
        .await
        .expect("seed address store");

    for account in extra {
        let sk = Fr::from_hex(account.secret_key).expect("valid extra secret key");
        let ca = imported_complete_address(*account);
        pxe.key_store()
            .add_account(&sk)
            .await
            .expect("seed extra key");
        pxe.address_store()
            .add(&ca)
            .await
            .expect("seed extra address");
        pxe.register_sender(&ca.address)
            .await
            .expect("register sender");
    }

    let compiled_account = load_schnorr_account_artifact();
    register_account_for_authwit(&pxe, &compiled_account, primary).await;
    for account in extra {
        register_account_for_authwit(&pxe, &compiled_account, *account).await;
    }

    let account_contract = SchnorrAccountContract::new(secret_key);
    seed_signing_key_note(&pxe, &account_contract, complete.address, 1).await;

    let provider = SingleAccountProvider::new(
        complete.clone(),
        Box::new(SchnorrAccountContract::new(secret_key)),
        primary.alias,
    );
    let wallet = Arc::new(BaseWallet::new(pxe, node, provider));
    Some((wallet, complete.address))
}

/// Creates a wallet for `primary` with `extra` accounts registered in the PXE
/// (for event decryption), but without authwit contract/note setup.
pub async fn setup_wallet_with_accounts(
    primary: ImportedTestAccount,
    extra: &[ImportedTestAccount],
) -> Option<(TestWallet, AztecAddress)> {
    let url = node_url();
    let node = create_aztec_node_client(&url);
    if node.get_node_info().await.is_err() {
        return None;
    }

    let kv = Arc::new(InMemoryKvStore::new());
    let pxe = EmbeddedPxe::create(node.clone(), kv).await.ok()?;

    let secret_key = Fr::from_hex(primary.secret_key).expect("valid test account secret key");
    let complete = imported_complete_address(primary);
    pxe.key_store()
        .add_account(&secret_key)
        .await
        .expect("seed key store for primary");
    pxe.address_store()
        .add(&complete)
        .await
        .expect("seed address store for primary");

    for account in extra {
        let sk = Fr::from_hex(account.secret_key).expect("valid extra account secret key");
        let ca = imported_complete_address(*account);
        pxe.key_store()
            .add_account(&sk)
            .await
            .expect("seed key store for extra account");
        pxe.address_store()
            .add(&ca)
            .await
            .expect("seed address store for extra account");
    }

    let account_contract = SchnorrAccountContract::new(secret_key);
    let provider =
        SingleAccountProvider::new(complete.clone(), Box::new(account_contract), primary.alias);
    let wallet = BaseWallet::new(pxe, node, provider);
    Some((wallet, complete.address))
}

// ---------------------------------------------------------------------------
// Contract deployment helpers
// ---------------------------------------------------------------------------

/// Registers a contract class and instance on a PXE.
pub async fn register_contract_on_pxe(
    pxe: &impl Pxe,
    artifact: &ContractArtifact,
    instance: &ContractInstanceWithAddress,
) {
    pxe.register_contract_class(artifact).await.ok();
    pxe.register_contract(RegisterContractRequest {
        instance: instance.clone(),
        artifact: Some(artifact.clone()),
    })
    .await
    .expect("register contract");
}

/// Deploys a contract and returns its address, artifact, and instance.
pub async fn deploy_contract(
    wallet: &impl Wallet,
    artifact: ContractArtifact,
    constructor_args: Vec<AbiValue>,
    from: AztecAddress,
) -> (AztecAddress, ContractArtifact, ContractInstanceWithAddress) {
    let result = Contract::deploy(wallet, artifact.clone(), constructor_args, None)
        .expect("deploy builder")
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

    (result.instance.address, artifact, result.instance)
}

/// Constructs a `ContractInstanceWithAddress` locally (without deploying).
/// Useful for registering pre-existing contracts on a PXE.
pub fn make_instance(artifact: &ContractArtifact, salt: u64) -> ContractInstanceWithAddress {
    let class_id = compute_contract_class_id_from_artifact(artifact).expect("compute class id");
    let inner = ContractInstance {
        version: 1,
        salt: Fr::from(salt),
        deployer: AztecAddress(Fr::zero()),
        current_contract_class_id: class_id,
        original_contract_class_id: class_id,
        initialization_hash: Fr::zero(),
        public_keys: PublicKeys::default(),
    };
    let address = compute_contract_address_from_instance(&inner).expect("compute address");
    ContractInstanceWithAddress { address, inner }
}

// ---------------------------------------------------------------------------
// Contract interaction helpers
// ---------------------------------------------------------------------------

/// Builds a [`FunctionCall`] by looking up `method_name` in the artifact.
pub fn build_call(
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
) -> FunctionCall {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|_| panic!("function '{method_name}' not found in artifact"));
    FunctionCall {
        to: contract_address,
        selector: func.selector.expect("selector"),
        args,
        function_type: func.function_type.clone(),
        is_static: func.is_static,
        hide_msg_sender: false,
    }
}

/// Sends a single contract method call as a transaction.
pub async fn send_token_method(
    wallet: &impl Wallet,
    artifact: &ContractArtifact,
    token_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    from: AztecAddress,
) {
    let call = build_call(artifact, token_address, method_name, args);
    wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from,
                ..Default::default()
            },
        )
        .await
        .expect("send tx");
}

/// Sends a single [`FunctionCall`] as a transaction.
pub async fn send_call(wallet: &impl Wallet, call: FunctionCall, from: AztecAddress) {
    wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from,
                ..Default::default()
            },
        )
        .await
        .expect("send tx");
}

/// Sends a call and asserts it fails with an error containing one of the
/// expected fragments (case-insensitive).
pub async fn send_call_should_fail(
    wallet: &impl Wallet,
    call: FunctionCall,
    from: AztecAddress,
    expected_fragments: &[&str],
) {
    let err = wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SendOptions {
                from,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail");

    let err_str = err.to_string().to_lowercase();
    let matches = expected_fragments
        .iter()
        .any(|frag| err_str.contains(&frag.to_lowercase()));
    assert!(
        matches,
        "expected one of {expected_fragments:?}, got: {err}",
    );
}

/// Simulates a call and asserts it fails with an error containing one of the
/// expected fragments (case-insensitive).
pub async fn simulate_should_fail(
    wallet: &impl Wallet,
    call: FunctionCall,
    from: AztecAddress,
    expected_fragments: &[&str],
) {
    let err = wallet
        .simulate_tx(
            ExecutionPayload {
                calls: vec![call],
                ..Default::default()
            },
            SimulateOptions {
                from,
                ..Default::default()
            },
        )
        .await
        .expect_err("should fail");

    let err_str = err.to_string().to_lowercase();
    let matches = expected_fragments
        .iter()
        .any(|frag| err_str.contains(&frag.to_lowercase()));
    assert!(
        matches,
        "expected one of {expected_fragments:?}, got: {err}",
    );
}

// ---------------------------------------------------------------------------
// Utility (view) call helpers
// ---------------------------------------------------------------------------

/// Execute a utility function and return the raw first-field value as `u64`.
#[allow(clippy::cast_possible_truncation)]
pub async fn call_utility_u64(
    wallet: &impl Wallet,
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    scope: AztecAddress,
) -> u64 {
    let func = artifact
        .find_function(method_name)
        .unwrap_or_else(|e| panic!("function '{method_name}' not found: {e}"));
    let call = FunctionCall {
        to: contract_address,
        selector: func.selector.expect("selector"),
        args,
        function_type: FunctionType::Utility,
        is_static: false,
        hide_msg_sender: false,
    };
    let result = wallet
        .execute_utility(
            call,
            ExecuteUtilityOptions {
                scope,
                auth_witnesses: vec![],
            },
        )
        .await
        .unwrap_or_else(|e| panic!("execute {method_name}: {e}"));

    result
        .result
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .and_then(|s| Fr::from_hex(s).ok())
        .map_or(0u64, |f| f.to_usize() as u64)
}

/// Execute a utility function and return the result as `u128`.
pub async fn call_utility_u128(
    wallet: &impl Wallet,
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    scope: AztecAddress,
) -> u128 {
    u128::from(call_utility_u64(wallet, artifact, contract_address, method_name, args, scope).await)
}

/// Execute a utility function and return the result as `bool`.
pub async fn call_utility_bool(
    wallet: &impl Wallet,
    artifact: &ContractArtifact,
    contract_address: AztecAddress,
    method_name: &str,
    args: Vec<AbiValue>,
    scope: AztecAddress,
) -> bool {
    call_utility_u64(wallet, artifact, contract_address, method_name, args, scope).await != 0
}

// ---------------------------------------------------------------------------
// Public storage helpers
// ---------------------------------------------------------------------------

/// Read a raw `Fr` from public storage at the given slot.
pub async fn read_public_storage(wallet: &TestWallet, contract: AztecAddress, slot: Fr) -> Fr {
    wallet
        .pxe()
        .node()
        .get_public_storage_at(0, &contract, &slot)
        .await
        .expect("get_public_storage_at")
}

/// Read a `u128` value from public storage at the given slot.
pub async fn read_public_u128(wallet: &TestWallet, contract: AztecAddress, slot: Fr) -> u128 {
    let raw = read_public_storage(wallet, contract, slot).await;
    let bytes = raw.to_be_bytes();
    u128::from_be_bytes(bytes[16..32].try_into().expect("16 bytes"))
}

// ---------------------------------------------------------------------------
// ABI encoding helpers
// ---------------------------------------------------------------------------

/// Wrap an [`AztecAddress`] into the ABI struct representation
/// `{ inner: Field }` expected by contract function arguments.
pub fn abi_address(address: AztecAddress) -> AbiValue {
    let mut fields = BTreeMap::new();
    fields.insert("inner".to_owned(), AbiValue::Field(Fr::from(address)));
    AbiValue::Struct(fields)
}

/// Wrap a [`FunctionSelector`] into the ABI struct representation
/// `{ inner: u32 }` expected by contract function arguments.
pub fn abi_selector(selector: FunctionSelector) -> AbiValue {
    let mut fields = BTreeMap::new();
    fields.insert(
        "inner".to_owned(),
        AbiValue::Integer(u32::from_be_bytes(selector.0).into()),
    );
    AbiValue::Struct(fields)
}

// ---------------------------------------------------------------------------
// Ethereum / L1 helpers
// ---------------------------------------------------------------------------

pub use aztec_rs::types::EthAddress;

/// Parse a hex string (with or without 0x) into a 20-byte Ethereum address.
#[allow(clippy::cast_possible_truncation)]
pub fn parse_eth_address(hex_str: &str) -> EthAddress {
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let mut bytes = [0u8; 20];
    let nibbles: Vec<u8> = hex_str
        .chars()
        .filter_map(|c| c.to_digit(16).map(|d| d as u8))
        .collect();
    let len = nibbles.len() / 2;
    let start = 20usize.saturating_sub(len);
    for i in 0..len.min(20) {
        bytes[start + i] = (nibbles[i * 2] << 4) | nibbles[i * 2 + 1];
    }
    EthAddress(bytes)
}

/// Convert an `EthAddress` to an `Fr` (left-padded to 32 bytes).
pub fn eth_address_as_field(addr: &EthAddress) -> Fr {
    let mut bytes = [0u8; 32];
    bytes[12..32].copy_from_slice(&addr.0);
    Fr::from(bytes)
}

// ---------------------------------------------------------------------------
// Proxy helpers
// ---------------------------------------------------------------------------

/// Build a proxy forwarding call (e.g. `forward_private_N`).
pub fn build_proxy_call(
    proxy_artifact: &ContractArtifact,
    proxy_address: AztecAddress,
    action: &FunctionCall,
) -> FunctionCall {
    let method_name = format!("forward_private_{}", action.args.len());
    build_call(
        proxy_artifact,
        proxy_address,
        &method_name,
        vec![
            abi_address(action.to),
            abi_selector(action.selector),
            AbiValue::Array(action.args.clone()),
        ],
    )
}

// ---------------------------------------------------------------------------
// Token-specific constants
// ---------------------------------------------------------------------------

/// Storage slot layout for the token contract (matches upstream Noir storage struct).
pub mod token_storage {
    /// `public_balances: Map<AztecAddress, PublicMutable<U128>>`
    pub const PUBLIC_BALANCES_SLOT: u64 = 5;
}

/// Read the public balance of an account from the token contract.
pub async fn public_balance(
    wallet: &TestWallet,
    token: AztecAddress,
    account: &AztecAddress,
) -> u128 {
    let slot = derive_storage_slot_in_map(token_storage::PUBLIC_BALANCES_SLOT, account);
    read_public_u128(wallet, token, slot).await
}

// ---------------------------------------------------------------------------
// Token deployment & interaction helpers
// ---------------------------------------------------------------------------

/// Deploy the compiled token contract, optionally minting an initial private
/// balance to `admin`. Mirrors upstream `TokenContract.deploy(admin, ...)`.
pub async fn deploy_token(
    wallet: &impl Wallet,
    admin: AztecAddress,
    initial_balance: u64,
) -> (AztecAddress, ContractArtifact, ContractInstanceWithAddress) {
    let artifact = load_token_artifact();
    let (token_address, artifact, instance) = deploy_contract(
        wallet,
        artifact,
        vec![
            AbiValue::Field(Fr::from(admin)),
            AbiValue::String("TestToken".to_owned()),
            AbiValue::String("TT".to_owned()),
            AbiValue::Integer(18),
        ],
        admin,
    )
    .await;

    if initial_balance > 0 {
        mint_tokens_to_private(
            wallet,
            token_address,
            &artifact,
            admin,
            admin,
            initial_balance,
        )
        .await;
    }

    (token_address, artifact, instance)
}

/// Call `mint_to_private` on a token contract.
pub async fn mint_tokens_to_private(
    wallet: &impl Wallet,
    token_address: AztecAddress,
    artifact: &ContractArtifact,
    from: AztecAddress,
    to: AztecAddress,
    amount: u64,
) {
    send_token_method(
        wallet,
        artifact,
        token_address,
        "mint_to_private",
        vec![
            AbiValue::Field(Fr::from(to)),
            AbiValue::Integer(i128::from(amount)),
        ],
        from,
    )
    .await;
}

/// Call `balance_of_private` and assert the result equals `expected`.
pub async fn expect_token_balance(
    wallet: &impl Wallet,
    token_address: AztecAddress,
    artifact: &ContractArtifact,
    owner: AztecAddress,
    expected: u64,
) {
    let balance = call_utility_u64(
        wallet,
        artifact,
        token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(owner))],
        owner,
    )
    .await;

    assert_eq!(
        balance, expected,
        "expected balance {expected} for {owner}, got {balance}"
    );
}

// ---------------------------------------------------------------------------
// Common token test state (shared across many token e2e tests)
// ---------------------------------------------------------------------------

/// Common shared state for token e2e tests: two wallets with a deployed token
/// contract and initial minting done.  Tests that need extra state (proxy,
/// bad_account, etc.) can embed this and add their own fields.
pub struct TokenTestState {
    pub admin_wallet: TestWallet,
    pub account1_wallet: TestWallet,
    pub admin_address: AztecAddress,
    pub account1_address: AztecAddress,
    pub token_address: AztecAddress,
    pub token_artifact: ContractArtifact,
}

/// Initialise a [`TokenTestState`]: two wallets (via `setup_wallet`) with
/// sender registration, a deployed token contract registered on both PXEs,
/// and configurable minting to admin.
pub async fn init_token_test_state(public_mint: u64, private_mint: u64) -> Option<TokenTestState> {
    let (admin_wallet, admin_address) = setup_wallet(TEST_ACCOUNT_0).await?;
    let (account1_wallet, account1_address) = setup_wallet(TEST_ACCOUNT_1).await?;

    // Register senders across wallets for tag discovery
    admin_wallet
        .pxe()
        .register_sender(&account1_address)
        .await
        .expect("admin registers account1");
    account1_wallet
        .pxe()
        .register_sender(&admin_address)
        .await
        .expect("account1 registers admin");

    // Deploy token with admin as the admin/minter
    let (token_address, token_artifact, token_instance) =
        deploy_token(&admin_wallet, admin_address, 0).await;

    // Register token on account1's PXE
    register_contract_on_pxe(account1_wallet.pxe(), &token_artifact, &token_instance).await;

    if public_mint > 0 {
        send_token_method(
            &admin_wallet,
            &token_artifact,
            token_address,
            "mint_to_public",
            vec![
                AbiValue::Field(Fr::from(admin_address)),
                AbiValue::Integer(i128::from(public_mint)),
            ],
            admin_address,
        )
        .await;
    }

    if private_mint > 0 {
        mint_tokens_to_private(
            &admin_wallet,
            token_address,
            &token_artifact,
            admin_address,
            admin_address,
            private_mint,
        )
        .await;
    }

    Some(TokenTestState {
        admin_wallet,
        account1_wallet,
        admin_address,
        account1_address,
        token_address,
        token_artifact,
    })
}

// ---------------------------------------------------------------------------
// Authwit token test state (uses create_wallet with Arc-wrapped wallets)
// ---------------------------------------------------------------------------

/// Shared state for token tests that need authwit support (Arc-wrapped
/// wallets from [`create_wallet`]).
pub struct AuthwitTokenTestState {
    pub admin_wallet: SharedTestWallet,
    pub account1_wallet: SharedTestWallet,
    pub admin_address: AztecAddress,
    pub account1_address: AztecAddress,
    pub token_address: AztecAddress,
    pub token_artifact: ContractArtifact,
}

/// Initialise an [`AuthwitTokenTestState`]: two authwit-capable wallets (via
/// `create_wallet`), a deployed token contract registered on both PXEs,
/// and configurable minting to admin.
pub async fn init_authwit_token_test_state(
    public_mint: u64,
    private_mint: u64,
) -> Option<AuthwitTokenTestState> {
    let (admin_wallet, admin_address) = create_wallet(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1]).await?;
    let (account1_wallet, account1_address) =
        create_wallet(TEST_ACCOUNT_1, &[TEST_ACCOUNT_0]).await?;

    let (token_address, token_artifact, token_instance) = deploy_contract(
        &*admin_wallet,
        load_token_artifact(),
        vec![
            AbiValue::Field(Fr::from(admin_address)),
            AbiValue::String("TestToken".to_owned()),
            AbiValue::String("TT".to_owned()),
            AbiValue::Integer(18),
        ],
        admin_address,
    )
    .await;

    register_contract_on_pxe(account1_wallet.pxe(), &token_artifact, &token_instance).await;

    if public_mint > 0 {
        send_token_method(
            &*admin_wallet,
            &token_artifact,
            token_address,
            "mint_to_public",
            vec![
                AbiValue::Field(Fr::from(admin_address)),
                AbiValue::Integer(i128::from(public_mint)),
            ],
            admin_address,
        )
        .await;
    }

    if private_mint > 0 {
        mint_tokens_to_private(
            &*admin_wallet,
            token_address,
            &token_artifact,
            admin_address,
            admin_address,
            private_mint,
        )
        .await;
    }

    Some(AuthwitTokenTestState {
        admin_wallet,
        account1_wallet,
        admin_address,
        account1_address,
        token_address,
        token_artifact,
    })
}
