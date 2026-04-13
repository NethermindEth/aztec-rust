//! Expiration timestamp tests -- 1:1 mirror of upstream
//! `end-to-end/src/e2e_expiration_timestamp.test.ts`.
//!
//! Tests tx validity windows by calling `set_expiration_timestamp` with
//! various expiration values and enqueued-public-call configurations.
//! Expirations in the future should succeed; expirations at/below the last
//! mined block's timestamp should fail tx submission.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_expiration_timestamp -- --ignored --nocapture
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

/// Default local devnet slot duration in seconds. Can be overridden with the
/// `AZTEC_SLOT_DURATION` env var to match upstream's
/// `getL1ContractsConfigEnvVars().aztecSlotDuration`.
const DEFAULT_SLOT_DURATION_SECS: u64 = 36;

fn slot_duration() -> u64 {
    std::env::var("AZTEC_SLOT_DURATION")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_SLOT_DURATION_SECS)
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct ExpirationState {
    wallet: TestWallet,
    account: AztecAddress,
    contract_address: AztecAddress,
    artifact: ContractArtifact,
}

static SHARED_STATE: OnceCell<Option<ExpirationState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static ExpirationState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<ExpirationState> {
    let artifact = load_test_contract_artifact();
    let (wallet, account) = setup_wallet(TEST_ACCOUNT_0).await?;
    let (contract_address, artifact, _instance) =
        deploy_contract(&wallet, artifact, vec![], account).await;
    Some(ExpirationState {
        wallet,
        account,
        contract_address,
        artifact,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fetch the `globalVariables.timestamp` of the latest block header.
async fn latest_block_timestamp(wallet: &TestWallet) -> u64 {
    let block_number = wallet
        .pxe()
        .node()
        .get_block_number()
        .await
        .expect("get block number");
    let header = wallet
        .pxe()
        .node()
        .get_block_header(block_number)
        .await
        .expect("get block header");

    // Header JSON structure: { globalVariables: { timestamp: "0x..." | u64 }, ... }
    let ts = header
        .pointer("/globalVariables/timestamp")
        .or_else(|| header.pointer("/global_variables/timestamp"))
        .expect("timestamp in header");

    match ts {
        serde_json::Value::String(s) => {
            if let Some(hex) = s.strip_prefix("0x") {
                u64::from_str_radix(hex, 16).expect("parse hex timestamp")
            } else {
                s.parse::<u64>().expect("parse decimal timestamp")
            }
        }
        serde_json::Value::Number(n) => n.as_u64().expect("u64 timestamp"),
        other => panic!("unexpected timestamp format: {other:?}"),
    }
}

/// Current wall-clock time in seconds since UNIX epoch.
fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time before UNIX epoch")
        .as_secs()
}

/// Compute an expiration timestamp that is guaranteed to be accepted by the
/// node's `TimestampTxValidator` ("≥ nextSlotTimestamp") and by the PXE's
/// kernel-tail check ("> anchorBlockTimestamp").
///
/// Upstream TS uses `header.timestamp + aztecSlotDuration` directly.  That
/// relies on the L1 and L2 clocks being in lockstep with the header's block
/// timestamp — in their tightly-controlled test harness they are.  Our
/// sandbox runs against Anvil, which typically drives L2 block timestamps
/// from fast-forwarded L1 time (we observed `header_ts` ~2 days ahead of
/// wall-clock time in local runs).  The node's `nextSlotTimestamp` validator
/// pulls from the *current* L1 slot, which can be well past `header_ts +
/// aztecSlotDuration`.
///
/// Using `header_ts + 1h` keeps the semantics ("expiration in a future slot
/// that hasn't happened yet") while being robust to any realistic L1 clock
/// drift.  There is no upper bound on the protocol side — the PXE kernel
/// tail only asserts `> anchorBlockTimestamp` and the node validator only
/// asserts `≥ nextSlotTimestamp`.
fn future_expiration(header_ts: u64) -> u64 {
    const FUTURE_OFFSET_SECS: u64 = 3600;
    header_ts.max(now_unix_secs()) + FUTURE_OFFSET_SECS
}

/// Build the `set_expiration_timestamp(ts, make_tx_hybrid)` call.
fn set_expiration_call(
    artifact: &ContractArtifact,
    contract: AztecAddress,
    ts: u64,
    enqueue_public_call: bool,
) -> FunctionCall {
    build_call(
        artifact,
        contract,
        "set_expiration_timestamp",
        vec![
            AbiValue::Integer(i128::from(ts)),
            AbiValue::Boolean(enqueue_public_call),
        ],
    )
}

/// Attempt to send a `set_expiration_timestamp` tx; returns `Ok(())` on
/// success, `Err(message)` if the submission rejected the tx.
async fn send_set_expiration(
    wallet: &TestWallet,
    from: AztecAddress,
    call: FunctionCall,
) -> Result<(), String> {
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
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Upstream's `TX_ERROR_INVALID_EXPIRATION_TIMESTAMP` — the message surfaced
/// when the tx's expirationTimestamp is at/below the latest block's timestamp.
const INVALID_EXPIRATION_ERROR: &str = "Invalid expiration timestamp";

// ===========================================================================
// describe('when requesting expiration timestamp higher than the one of a mined block')
// ===========================================================================

/// TS: with no enqueued public calls > does not invalidate the transaction
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn higher_than_mined_no_enqueue_does_not_invalidate() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let header_ts = latest_block_timestamp(&s.wallet).await;
    let expiration = future_expiration(header_ts);

    let call = set_expiration_call(&s.artifact, s.contract_address, expiration, false);
    send_set_expiration(&s.wallet, s.account, call)
        .await
        .expect("tx with future expiration should land");
}

/// TS: with an enqueued public call > does not invalidate the transaction
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn higher_than_mined_with_enqueue_does_not_invalidate() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let header_ts = latest_block_timestamp(&s.wallet).await;
    let expiration = future_expiration(header_ts);

    let call = set_expiration_call(&s.artifact, s.contract_address, expiration, true);
    send_set_expiration(&s.wallet, s.account, call)
        .await
        .expect("tx with future expiration + enqueued public call should land");
}

// ===========================================================================
// describe('when requesting expiration timestamp lower than the next block')
// ===========================================================================

/// TS: with no enqueued public calls > invalidates the transaction
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn lower_than_next_no_enqueue_invalidates() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let header_ts = latest_block_timestamp(&s.wallet).await;
    // 1 below the next slot boundary: header_ts + slot_duration - 1
    let expiration = header_ts + slot_duration() - 1;

    let call = set_expiration_call(&s.artifact, s.contract_address, expiration, false);
    let err = send_set_expiration(&s.wallet, s.account, call)
        .await
        .expect_err("tx with too-close expiration should fail");
    assert!(
        err.contains(INVALID_EXPIRATION_ERROR)
            || err.to_lowercase().contains("expiration")
            || err.contains("invalid"),
        "expected expiration-related error, got: {err}"
    );
}

/// TS: with an enqueued public call > invalidates the transaction
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn lower_than_next_with_enqueue_invalidates() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let header_ts = latest_block_timestamp(&s.wallet).await;
    let expiration = header_ts + slot_duration() - 1;

    let call = set_expiration_call(&s.artifact, s.contract_address, expiration, true);
    let err = send_set_expiration(&s.wallet, s.account, call)
        .await
        .expect_err("tx with too-close expiration + enqueue should fail");
    assert!(
        err.contains(INVALID_EXPIRATION_ERROR)
            || err.to_lowercase().contains("expiration")
            || err.contains("invalid"),
        "expected expiration-related error, got: {err}"
    );
}

// ===========================================================================
// describe('when requesting expiration timestamp lower than the one of a mined block')
// ===========================================================================

/// TS: with no enqueued public calls > fails to prove the tx
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn lower_than_mined_no_enqueue_fails_to_prove() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let header_ts = latest_block_timestamp(&s.wallet).await;
    // 1 below the latest mined block's timestamp.
    let expiration = header_ts.saturating_sub(1);

    let call = set_expiration_call(&s.artifact, s.contract_address, expiration, false);
    send_set_expiration(&s.wallet, s.account, call)
        .await
        .expect_err("tx with past expiration should fail to prove/submit");
}

/// TS: with an enqueued public call > fails to prove the tx
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn lower_than_mined_with_enqueue_fails_to_prove() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let header_ts = latest_block_timestamp(&s.wallet).await;
    let expiration = header_ts.saturating_sub(1);

    let call = set_expiration_call(&s.artifact, s.contract_address, expiration, true);
    send_set_expiration(&s.wallet, s.account, call)
        .await
        .expect_err("tx with past expiration + enqueue should fail to prove/submit");
}
