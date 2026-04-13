//! Token unified transfer tests -- 1:1 mirror of upstream
//! `end-to-end/src/e2e_token_contract/transfer.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_contract_transfer -- --ignored --nocapture
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

use aztec_rs::abi::{AbiParameter, AbiType, EventSelector, FunctionSelector};
use aztec_rs::wallet::{EventMetadataDefinition, PrivateEventFilter};

/// Mint amount used by setup (mirrors upstream `const amount = 10000n`).
const MINT_AMOUNT: u64 = 10000;

const TOKEN_NAME: &str = "TestToken";
const TOKEN_SYMBOL: &str = "TT";
const TOKEN_DECIMALS: u8 = 18;

// ---------------------------------------------------------------------------
// Shared test state (mirrors beforeAll in upstream TokenContractTest)
// ---------------------------------------------------------------------------

static SHARED_STATE: OnceCell<Option<TokenTestState>> = OnceCell::const_new();

async fn get_shared_state() -> Option<&'static TokenTestState> {
    SHARED_STATE
        .get_or_init(|| async { init_token_test_state(0, MINT_AMOUNT).await })
        .await
        .as_ref()
}

/// Event selector for `Transfer((Field),(Field),u128)`.
fn transfer_event_metadata() -> EventMetadataDefinition {
    EventMetadataDefinition {
        event_selector: EventSelector(
            FunctionSelector::from_signature("Transfer((Field),(Field),u128)").to_field(),
        ),
        abi_type: AbiType::Struct {
            name: "Transfer".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "from".to_owned(),
                    typ: AbiType::Struct {
                        name: "AztecAddress".to_owned(),
                        fields: vec![AbiParameter {
                            name: "inner".to_owned(),
                            typ: AbiType::Field,
                            visibility: None,
                        }],
                    },
                    visibility: None,
                },
                AbiParameter {
                    name: "to".to_owned(),
                    typ: AbiType::Struct {
                        name: "AztecAddress".to_owned(),
                        fields: vec![AbiParameter {
                            name: "inner".to_owned(),
                            typ: AbiType::Field,
                            visibility: None,
                        }],
                    },
                    visibility: None,
                },
                AbiParameter {
                    name: "amount".to_owned(),
                    typ: AbiType::Integer {
                        sign: "unsigned".to_owned(),
                        width: 128,
                    },
                    visibility: None,
                },
            ],
        },
        field_names: vec!["from".to_owned(), "to".to_owned(), "amount".to_owned()],
    }
}

// ===========================================================================
// Tests: e2e_token_contract transfer private
// ===========================================================================

/// TS: transfer less than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_less_than_balance() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    assert!(amount > 0, "amount should be greater than 0");

    let tx_receipt = s
        .admin_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &s.token_artifact,
                    s.token_address,
                    "transfer",
                    vec![
                        AbiValue::Field(Fr::from(s.account1_address)),
                        AbiValue::Integer(amount as i128),
                    ],
                )],
                ..Default::default()
            },
            SendOptions {
                from: s.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect("send transfer");

    let receipt = s
        .admin_wallet
        .node()
        .get_tx_receipt(&tx_receipt.tx_hash)
        .await
        .expect("get receipt");
    let block_number = receipt.block_number.expect("block number");

    // Verify the Transfer private event was emitted and is decryptable by account1
    let events = s
        .account1_wallet
        .get_private_events(
            &transfer_event_metadata(),
            PrivateEventFilter {
                contract_address: s.token_address,
                from_block: Some(block_number),
                to_block: Some(block_number + 1),
                scopes: vec![s.account1_address],
                ..Default::default()
            },
        )
        .await
        .expect("get private Transfer events");

    assert!(!events.is_empty(), "expected at least one Transfer event");
    let event = &events[0].event;
    let obj = event.as_object().expect("event is object");

    let from_str = obj["from"].as_str().expect("from str");
    let to_str = obj["to"].as_str().expect("to str");
    let amount_str = obj["amount"].as_str().expect("amount str");

    let from_field = Fr::from_hex(from_str).expect("parse from");
    let to_field = Fr::from_hex(to_str).expect("parse to");
    let amount_field = Fr::from_hex(amount_str).expect("parse amount");

    assert_eq!(from_field, Fr::from(s.admin_address), "from mismatch");
    assert_eq!(to_field, Fr::from(s.account1_address), "to mismatch");
    assert_eq!(
        amount_field,
        Fr::from(amount),
        "amount mismatch in Transfer event"
    );
}

/// TS: transfer less than balance to non-deployed account
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_less_than_balance_to_non_deployed_account() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    assert!(amount > 0, "amount should be greater than 0");

    // A pseudo-random address that isn't deployed or known to any PXE
    let non_deployed = AztecAddress(Fr::random());

    s.admin_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &s.token_artifact,
                    s.token_address,
                    "transfer",
                    vec![
                        AbiValue::Field(Fr::from(non_deployed)),
                        AbiValue::Integer(amount as i128),
                    ],
                )],
                ..Default::default()
            },
            SendOptions {
                from: s.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect("transfer to non-deployed recipient should succeed");

    // The admin's balance should have decreased; we cannot assert the recipient's
    // balance since we don't hold their keys (upstream simulates this as a
    // transfer to address(0)).
    let admin_after = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    assert_eq!(
        admin_after,
        balance0 - amount,
        "admin balance should have decreased by the transferred amount"
    );
}

/// TS: transfer to self
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_to_self() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = balance0 / 2;
    assert!(amount > 0, "amount should be greater than 0");

    s.admin_wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &s.token_artifact,
                    s.token_address,
                    "transfer",
                    vec![
                        AbiValue::Field(Fr::from(s.admin_address)),
                        AbiValue::Integer(amount as i128),
                    ],
                )],
                ..Default::default()
            },
            SendOptions {
                from: s.admin_address,
                ..Default::default()
            },
        )
        .await
        .expect("self transfer");

    let admin_after = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    assert_eq!(
        admin_after, balance0,
        "self transfer must preserve total balance"
    );
}

// ---------------------------------------------------------------------------
// failure cases
// ---------------------------------------------------------------------------

/// TS: failure cases > transfer more than balance
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn transfer_more_than_balance() {
    let _guard = serial_guard();
    let Some(s) = get_shared_state().await else {
        return;
    };

    let balance0 = call_utility_u128(
        &s.admin_wallet,
        &s.token_artifact,
        s.token_address,
        "balance_of_private",
        vec![AbiValue::Field(Fr::from(s.admin_address))],
        s.admin_address,
    )
    .await;
    let amount = balance0 + 1;
    assert!(amount > 0, "amount should be greater than 0");

    let call = build_call(
        &s.token_artifact,
        s.token_address,
        "transfer",
        vec![
            AbiValue::Field(Fr::from(s.account1_address)),
            AbiValue::Integer(amount as i128),
        ],
    );

    simulate_should_fail(
        &s.admin_wallet,
        call,
        s.admin_address,
        &[
            "Balance too low",
            "Assertion failed",
            "Cannot satisfy constraint",
        ],
    )
    .await;
}
