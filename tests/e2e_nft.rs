//! NFT contract tests -- 1:1 mirror of upstream
//! `end-to-end/src/e2e_nft.test.ts`.
//!
//! NOTE: upstream uses 4 accounts (admin, minter, user1, user2); we collapse
//! admin+minter into a single account since only 3 pre-imported test accounts
//! are available. The `set_minter` step still exercises the same contract
//! entrypoint.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_nft -- --ignored --nocapture
//! ```

#![allow(
    clippy::await_holding_lock,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::similar_names,
    clippy::cast_possible_wrap,
    clippy::too_many_lines,
    dead_code,
    unused_imports
)]

mod common;
use common::*;

const NFT_NAME: &str = "FROG";
const NFT_SYMBOL: &str = "FRG";

// ---------------------------------------------------------------------------
// Shared state (each test relies on state built up by earlier ones)
// ---------------------------------------------------------------------------

struct NftState {
    admin_wallet: TestWallet,
    user1_wallet: TestWallet,
    user2_wallet: TestWallet,
    admin: AztecAddress,
    minter: AztecAddress,
    user1: AztecAddress,
    user2: AztecAddress,
    contract_address: AztecAddress,
    artifact: ContractArtifact,
    token_id: Fr,
}

static SHARED_STATE: OnceCell<Option<NftState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static NftState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<NftState> {
    let artifact = load_nft_artifact()?;

    let (admin_wallet, admin) =
        setup_wallet_with_accounts(TEST_ACCOUNT_0, &[TEST_ACCOUNT_1, TEST_ACCOUNT_2]).await?;
    let (user1_wallet, user1) =
        setup_wallet_with_accounts(TEST_ACCOUNT_1, &[TEST_ACCOUNT_0, TEST_ACCOUNT_2]).await?;
    let (user2_wallet, user2) =
        setup_wallet_with_accounts(TEST_ACCOUNT_2, &[TEST_ACCOUNT_0, TEST_ACCOUNT_1]).await?;

    // Register cross-PXE senders for tag discovery
    for (w, senders) in [
        (&admin_wallet, vec![user1, user2]),
        (&user1_wallet, vec![admin, user2]),
        (&user2_wallet, vec![admin, user1]),
    ] {
        for sender in senders {
            w.pxe().register_sender(&sender).await.ok();
        }
    }

    // Deploy the NFT contract
    let (contract_address, artifact, instance) = deploy_contract(
        &admin_wallet,
        artifact,
        vec![
            AbiValue::Field(Fr::from(admin)),
            AbiValue::String(NFT_NAME.to_owned()),
            AbiValue::String(NFT_SYMBOL.to_owned()),
        ],
        admin,
    )
    .await;

    // Register the contract on every PXE that interacts with it
    register_contract_on_pxe(user1_wallet.pxe(), &artifact, &instance).await;
    register_contract_on_pxe(user2_wallet.pxe(), &artifact, &instance).await;

    Some(NftState {
        admin_wallet,
        user1_wallet,
        user2_wallet,
        admin,
        // minter collapsed into admin since only 3 test accounts exist
        minter: admin,
        user1,
        user2,
        contract_address,
        artifact,
        // Arbitrary non-zero token id (mirrors upstream `Fr.random().toBigInt()`)
        token_id: Fr::random(),
    })
}

// ---------------------------------------------------------------------------
// Test-specific helpers
// ---------------------------------------------------------------------------

// NFT storage slots (from compiled artifact's outputs.globals.storage).
// These mirror the Noir contract's `#[storage] struct Storage<Context>` order.
const NFT_SLOT_MINTERS: u64 = 6;
const NFT_SLOT_PUBLIC_OWNERS: u64 = 9;

const DOM_SEP_PUBLIC_STORAGE_MAP_SLOT: u32 = 4_015_149_901;

/// Derive the public storage slot of `map[key]` for a `Map<Fr, _>`.
/// Mirrors `derive_storage_slot_in_map` in common but takes a `Fr` key.
fn map_slot_fr(base_slot: u64, key: &Fr) -> Fr {
    aztec_rs::hash::poseidon2_hash_with_separator(
        &[Fr::from(base_slot), *key],
        DOM_SEP_PUBLIC_STORAGE_MAP_SLOT,
    )
}

/// Read `nftContract.public_owners[token_id]` from public storage.
///
/// Upstream calls `nftContract.methods.owner_of(TOKEN_ID).simulate({ from })`,
/// which runs the public view via AVM simulation.  Public functions in the
/// compiled NFT artifact ship as transpiled AVM bytecode (`abi_public`
/// attribute, not `abi_utility`), so we cannot run them through the Rust PXE's
/// ACIR utility executor.  A direct public-storage read on the `public_owners`
/// map is the semantic equivalent for a view function that just returns a map
/// entry.
async fn owner_of(wallet: &TestWallet, contract: AztecAddress, token_id: Fr) -> AztecAddress {
    let slot = map_slot_fr(NFT_SLOT_PUBLIC_OWNERS, &token_id);
    let raw = read_public_storage(wallet, contract, slot).await;
    AztecAddress(raw)
}

/// Read `nftContract.minters[minter]` from public storage (true iff non-zero).
///
/// Same reason as `owner_of`: `is_minter` is a public view function with
/// transpiled AVM bytecode, so we bypass it and read the underlying map.
async fn is_minter(wallet: &TestWallet, contract: AztecAddress, minter: AztecAddress) -> bool {
    let slot = derive_storage_slot_in_map(NFT_SLOT_MINTERS, &minter);
    let raw = read_public_storage(wallet, contract, slot).await;
    raw != Fr::zero()
}

/// Returns the list of (non-zero) private NFT token IDs owned by `owner`.
async fn get_private_nfts(
    wallet: &TestWallet,
    artifact: &ContractArtifact,
    contract: AztecAddress,
    owner: AztecAddress,
) -> Vec<Fr> {
    let func = artifact
        .find_function("get_private_nfts")
        .expect("get_private_nfts function");
    let call = FunctionCall {
        to: contract,
        selector: func.selector.expect("selector"),
        args: vec![AbiValue::Field(Fr::from(owner)), AbiValue::Integer(0)],
        function_type: FunctionType::Utility,
        is_static: false,
        hide_msg_sender: false,
    };
    let result = wallet
        .execute_utility(
            call,
            ExecuteUtilityOptions {
                scope: owner,
                auth_witnesses: vec![],
            },
        )
        .await
        .expect("execute get_private_nfts");

    // Returns a tuple ([Field; MAX_NOTES_PER_PAGE], bool).  The JSON encoding
    // is generally a flat array; we take the leading array of field strings
    // and drop trailing zeros.  The final element is the page_limit_reached
    // flag.
    let arr = result
        .result
        .as_array()
        .expect("get_private_nfts result array");

    // Handle common representations:
    //   [[<id>, <id>, ...], bool]  -- tuple
    //   [<id>, <id>, ..., bool]    -- flattened
    let Some(first) = arr.first() else {
        return vec![];
    };

    let items: &[serde_json::Value] = first.as_array().map_or_else(
        || {
            // Flat shape; last element is the bool flag.
            &arr[..arr.len().saturating_sub(1)]
        },
        |inner| {
            // Tuple shape; the outer array's last element is the bool flag.
            if let Some(last) = arr.last() {
                assert!(
                    last.as_bool() != Some(true),
                    "page limit reached and pagination not implemented in test"
                );
            }
            inner.as_slice()
        },
    );

    items
        .iter()
        .filter_map(|v| v.as_str().and_then(|s| Fr::from_hex(s).ok()))
        .filter(|f| *f != Fr::zero())
        .collect()
}

// ===========================================================================
// Test (sequential; each step builds on the previous)
// ===========================================================================
//
// Upstream wraps the six `it(...)` blocks inside a single `describe('NFT', ...)`
// and relies on jest's sequential execution within a describe block.  In Rust,
// `#[tokio::test]` functions run in parallel by default, and `serial_guard()`
// only provides mutual exclusion, not ordering — so splitting the scenario
// across six test functions breaks when cargo interleaves them in the wrong
// order.
//
// To preserve the sequential semantics 1:1 we run all six steps inside one
// `#[tokio::test]` function.  Each step is clearly labelled so the mapping
// back to the TS `it(...)` blocks stays obvious.

#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn nft_lifecycle() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    // ---- TS: `it('sets minter', ...)` -------------------------------------
    send_token_method(
        &s.admin_wallet,
        &s.artifact,
        s.contract_address,
        "set_minter",
        vec![AbiValue::Field(Fr::from(s.minter)), AbiValue::Boolean(true)],
        s.admin,
    )
    .await;

    let is_minter_ok = is_minter(&s.admin_wallet, s.contract_address, s.minter).await;
    assert!(
        is_minter_ok,
        "minter should have the minter role after set_minter"
    );

    // ---- TS: `it('minter mints to a user', ...)` --------------------------
    send_token_method(
        &s.admin_wallet,
        &s.artifact,
        s.contract_address,
        "mint",
        vec![
            AbiValue::Field(Fr::from(s.user1)),
            AbiValue::Field(s.token_id),
        ],
        s.minter,
    )
    .await;

    let owner = owner_of(&s.user1_wallet, s.contract_address, s.token_id).await;
    assert_eq!(owner, s.user1, "user1 should own the token after mint");

    // ---- TS: `it('transfers to private', ...)` ----------------------------
    let recipient = s.user2;
    send_token_method(
        &s.user1_wallet,
        &s.artifact,
        s.contract_address,
        "transfer_to_private",
        vec![
            AbiValue::Field(Fr::from(recipient)),
            AbiValue::Field(s.token_id),
        ],
        s.user1,
    )
    .await;

    let public_owner = owner_of(&s.user1_wallet, s.contract_address, s.token_id).await;
    assert_eq!(
        public_owner,
        AztecAddress(Fr::zero()),
        "public owner should be zero after transfer_to_private"
    );

    // ---- TS: `it('transfers in private', ...)` ----------------------------
    // user2 transfers in private back to user1.
    send_token_method(
        &s.user2_wallet,
        &s.artifact,
        s.contract_address,
        "transfer_in_private",
        vec![
            AbiValue::Field(Fr::from(s.user2)),
            AbiValue::Field(Fr::from(s.user1)),
            AbiValue::Field(s.token_id),
            AbiValue::Field(Fr::zero()),
        ],
        s.user2,
    )
    .await;

    let user1_nfts =
        get_private_nfts(&s.user1_wallet, &s.artifact, s.contract_address, s.user1).await;
    assert_eq!(
        user1_nfts,
        vec![s.token_id],
        "user1 should now privately hold the token"
    );

    let user2_nfts =
        get_private_nfts(&s.user2_wallet, &s.artifact, s.contract_address, s.user2).await;
    assert!(
        user2_nfts.is_empty(),
        "user2 should have no private nfts after transferring out"
    );

    // ---- TS: `it('transfers to public', ...)` -----------------------------
    // user1 transfers to public (from=user1, to=user2).
    send_token_method(
        &s.user1_wallet,
        &s.artifact,
        s.contract_address,
        "transfer_to_public",
        vec![
            AbiValue::Field(Fr::from(s.user1)),
            AbiValue::Field(Fr::from(s.user2)),
            AbiValue::Field(s.token_id),
            AbiValue::Field(Fr::zero()),
        ],
        s.user1,
    )
    .await;

    let public_owner = owner_of(&s.user1_wallet, s.contract_address, s.token_id).await;
    assert_eq!(
        public_owner, s.user2,
        "public owner should be user2 after transfer_to_public"
    );

    // ---- TS: `it('transfers in public', ...)` -----------------------------
    send_token_method(
        &s.user2_wallet,
        &s.artifact,
        s.contract_address,
        "transfer_in_public",
        vec![
            AbiValue::Field(Fr::from(s.user2)),
            AbiValue::Field(Fr::from(s.user1)),
            AbiValue::Field(s.token_id),
            AbiValue::Field(Fr::zero()),
        ],
        s.user2,
    )
    .await;

    let public_owner = owner_of(&s.user2_wallet, s.contract_address, s.token_id).await;
    assert_eq!(
        public_owner, s.user1,
        "public owner should be user1 after transfer_in_public"
    );
}
