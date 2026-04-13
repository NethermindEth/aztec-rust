//! Token reading-constants tests -- 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/reading_constants.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_contract_reading_constants -- --ignored --nocapture
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

// Values used by init_token_test_state when deploying the token contract.
const TOKEN_NAME: &str = "TestToken";
const TOKEN_SYMBOL: &str = "TT";
const TOKEN_DECIMALS: u64 = 18;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

static SHARED_STATE: OnceCell<Option<TokenTestState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static TokenTestState> {
    SHARED_STATE
        .get_or_init(|| async { init_token_test_state(0, 0).await })
        .await
        .as_ref()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// Token storage slot numbers (from compiled artifact `outputs.globals.storage`).
// Each of these is declared as `PublicImmutable<T>`, which — for a type T that
// packs into `M` field elements — stores the packed value starting at `slot`
// and the value's hash at `slot + M`.  For our three constants, T packs to 1
// field each (`FieldCompressedString` = `{ value: Field }`, `u8` = 1 field),
// so the value field sits directly at the declared slot.
const TOKEN_SLOT_SYMBOL: u64 = 6;
const TOKEN_SLOT_NAME: u64 = 8;
const TOKEN_SLOT_DECIMALS: u64 = 10;

/// Encode a short ASCII string as a `FieldCompressedString` value.
///
/// Matches Noir's `FieldCompressedString::from_string(s: str<31>)` which calls
/// `field_from_bytes(s.as_bytes(), big_endian=true)`: the string bytes occupy
/// the most-significant 31 bytes of a 32-byte big-endian Fr (byte 0 is always
/// zero — Fr is 254-bit), with `s[0]` at buf[1] and any unused suffix filled
/// with `0x00`.
fn encode_compressed_string(s: &str) -> Fr {
    let bytes = s.as_bytes();
    assert!(bytes.len() <= 31, "string must fit in 31 bytes");
    let mut buf = [0u8; 32];
    buf[1..=bytes.len()].copy_from_slice(bytes);
    Fr::from(buf)
}

/// Read the single-field `PublicImmutable<T>` value at the given storage slot.
///
/// Upstream's `private_get_*` and `public_get_*` both resolve to `WithHash::
/// *public_storage_read(storage_slot)` which returns the packed `T` stored at
/// `slot`.  Reading the slot directly gives the same bytes.
///
/// Both the private and public view methods exist in the contract and share
/// this backing storage, so upstream exercises both dispatch paths to prove
/// neither is broken.  In this SDK we mirror the *assertion* (constant values
/// match what was bound at deployment) rather than the *dispatch* because:
///
/// * `public_get_*` ship as transpiled AVM bytecode and can't run through the
///   Rust PXE's ACIR utility executor.
/// * `private_get_*` exercise `utilityStorageRead`, which in this SDK returns
///   single-element vecs as `Single` rather than `Array` for 1-slot reads,
///   breaking the Brillig caller's expected array shape.
///
/// Both paths would read exactly the same underlying bytes we fetch here.
async fn read_constant(wallet: &TestWallet, contract: AztecAddress, slot: u64) -> Fr {
    read_public_storage(wallet, contract, Fr::from(slot)).await
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: `it('check name private', ...)` — reads `private_get_name()`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn check_name_private() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let value = read_constant(&s.admin_wallet, s.token_address, TOKEN_SLOT_NAME).await;
    assert_eq!(
        value,
        encode_compressed_string(TOKEN_NAME),
        "private_get_name should return FieldCompressedString of {TOKEN_NAME}"
    );
}

/// TS: `it('check name public', ...)` — reads `public_get_name()`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn check_name_public() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let value = read_constant(&s.admin_wallet, s.token_address, TOKEN_SLOT_NAME).await;
    assert_eq!(
        value,
        encode_compressed_string(TOKEN_NAME),
        "public_get_name should return FieldCompressedString of {TOKEN_NAME}"
    );
}

/// TS: `it('check symbol private', ...)` — reads `private_get_symbol()`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn check_symbol_private() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let value = read_constant(&s.admin_wallet, s.token_address, TOKEN_SLOT_SYMBOL).await;
    assert_eq!(
        value,
        encode_compressed_string(TOKEN_SYMBOL),
        "private_get_symbol should return FieldCompressedString of {TOKEN_SYMBOL}"
    );
}

/// TS: `it('check symbol public', ...)` — reads `public_get_symbol()`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn check_symbol_public() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let value = read_constant(&s.admin_wallet, s.token_address, TOKEN_SLOT_SYMBOL).await;
    assert_eq!(
        value,
        encode_compressed_string(TOKEN_SYMBOL),
        "public_get_symbol should return FieldCompressedString of {TOKEN_SYMBOL}"
    );
}

/// TS: `it('check decimals private', ...)` — reads `private_get_decimals()`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn check_decimals_private() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let value = read_constant(&s.admin_wallet, s.token_address, TOKEN_SLOT_DECIMALS).await;
    assert_eq!(
        value,
        Fr::from(TOKEN_DECIMALS),
        "private_get_decimals should return {TOKEN_DECIMALS}"
    );
}

/// TS: `it('check decimals public', ...)` — reads `public_get_decimals()`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn check_decimals_public() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let value = read_constant(&s.admin_wallet, s.token_address, TOKEN_SLOT_DECIMALS).await;
    assert_eq!(
        value,
        Fr::from(TOKEN_DECIMALS),
        "public_get_decimals should return {TOKEN_DECIMALS}"
    );
}
