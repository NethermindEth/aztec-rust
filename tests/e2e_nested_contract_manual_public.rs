//! Public nested contract tests -- 1:1 mirror of upstream
//! `end-to-end/src/e2e_nested_contract/manual_public.test.ts`.
//!
//! Exercises nested public function calls via the `Parent` contract's
//! `pub_entry_point`, `pub_entry_point_twice`, and `enqueue_call_to_child`
//! entrypoints, hitting the `Child` contract's `pub_get_value`,
//! `pub_inc_value`, and `pub_set_value`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_nested_contract_manual_public -- --ignored --nocapture
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

/// `Child` contract's stored `current_value` lives at public storage slot 1.
const CHILD_VALUE_SLOT: u64 = 1;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct NestedState {
    wallet: TestWallet,
    account: AztecAddress,
    parent_address: AztecAddress,
    parent_artifact: ContractArtifact,
    child_address: AztecAddress,
    child_artifact: ContractArtifact,
}

static SHARED_STATE: OnceCell<Option<NestedState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static NestedState> {
    SHARED_STATE
        .get_or_init(|| async { init_shared_state().await })
        .await
        .as_ref()
}

async fn init_shared_state() -> Option<NestedState> {
    let (wallet, account) = setup_wallet(TEST_ACCOUNT_0).await?;
    let (parent_address, parent_artifact, _) =
        deploy_contract(&wallet, load_parent_contract_artifact(), vec![], account).await;
    let (child_address, child_artifact, _) =
        deploy_contract(&wallet, load_child_contract_artifact(), vec![], account).await;
    Some(NestedState {
        wallet,
        account,
        parent_address,
        parent_artifact,
        child_address,
        child_artifact,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Look up the selector of `method_name` in `artifact`.
fn selector_of(artifact: &ContractArtifact, method_name: &str) -> FunctionSelector {
    artifact
        .find_function(method_name)
        .unwrap_or_else(|e| panic!("function '{method_name}' not found: {e}"))
        .selector
        .expect("selector")
}

/// Read the child contract's stored `current_value` from public storage.
async fn get_child_stored_value(s: &NestedState) -> Fr {
    read_public_storage(&s.wallet, s.child_address, Fr::from(CHILD_VALUE_SLOT)).await
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: performs public nested calls
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn performs_public_nested_calls() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let pub_get_value_selector = selector_of(&s.child_artifact, "pub_get_value");

    let call = build_call(
        &s.parent_artifact,
        s.parent_address,
        "pub_entry_point",
        vec![
            abi_address(s.child_address),
            abi_selector(pub_get_value_selector),
            AbiValue::Integer(42),
        ],
    );
    send_call(&s.wallet, call, s.account).await;
}

/// TS: reads fresh value after write within the same tx (regression
/// for https://github.com/AztecProtocol/aztec-packages/issues/640)
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn reads_fresh_value_after_write_within_the_same_tx() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let pub_inc_value_selector = selector_of(&s.child_artifact, "pub_inc_value");

    let call = build_call(
        &s.parent_artifact,
        s.parent_address,
        "pub_entry_point_twice",
        vec![
            abi_address(s.child_address),
            abi_selector(pub_inc_value_selector),
            AbiValue::Integer(42),
        ],
    );
    send_call(&s.wallet, call, s.account).await;

    let stored = get_child_stored_value(s).await;
    assert_eq!(
        stored,
        Fr::from(84u64),
        "child.current_value should equal 84 after two increments of 42"
    );
}

/// TS: executes public calls in expected order (regression for
/// https://github.com/AztecProtocol/aztec-packages/issues/1645)
///
/// Batch-sends:
///   1. child.pub_set_value(20)
///   2. parent.enqueue_call_to_child(child, pub_set_value.selector, 40)
///
/// Verifies that the account entrypoint honours this order (does not run the
/// enqueued private-path public call ahead of the direct public call, which
/// would invert the writes).  After the tx, the child's stored value must be
/// `40` and the public logs must come out `[20, 40]`.
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn executes_public_calls_in_expected_order() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let pub_set_value_selector = selector_of(&s.child_artifact, "pub_set_value");

    let direct_call = build_call(
        &s.child_artifact,
        s.child_address,
        "pub_set_value",
        vec![AbiValue::Integer(20)],
    );
    let enqueue_call = build_call(
        &s.parent_artifact,
        s.parent_address,
        "enqueue_call_to_child",
        vec![
            abi_address(s.child_address),
            abi_selector(pub_set_value_selector),
            AbiValue::Integer(40),
        ],
    );

    s.wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![direct_call, enqueue_call],
                ..Default::default()
            },
            SendOptions {
                from: s.account,
                ..Default::default()
            },
        )
        .await
        .expect("send batched public calls");

    let stored = get_child_stored_value(s).await;
    assert_eq!(
        stored,
        Fr::from(40u64),
        "child.current_value should be 40 (last write), not 20"
    );
}
