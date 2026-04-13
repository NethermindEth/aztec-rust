//! EventOnly contract test -- 1:1 mirror of upstream
//! `end-to-end/src/e2e_event_only.test.ts`.
//!
//! Run with:
//! ```bash
//! AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_event_only -- --ignored --nocapture
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

use crate::common::*;

use aztec_rs::abi::{AbiParameter, AbiType, EventSelector};
use aztec_rs::wallet::{EventMetadataDefinition, PrivateEventFilter};

// ---------------------------------------------------------------------------
// Event metadata for TestEvent { value: Field }
// ---------------------------------------------------------------------------

fn test_event_metadata() -> EventMetadataDefinition {
    EventMetadataDefinition {
        event_selector: EventSelector(
            FunctionSelector::from_signature("TestEvent(Field)").to_field(),
        ),
        abi_type: AbiType::Struct {
            name: "TestEvent".to_owned(),
            fields: vec![AbiParameter {
                name: "value".to_owned(),
                typ: AbiType::Field,
                visibility: None,
            }],
        },
        field_names: vec!["value".to_owned()],
    }
}

// ===========================================================================
// Tests
// ===========================================================================

/// TS: emits and retrieves a private event for a contract with no notes
#[tokio::test]
#[ignore = "requires live node via AZTEC_NODE_URL"]
async fn emits_and_retrieves_a_private_event_for_a_contract_with_no_notes() {
    let _guard = serial_guard();

    let Some(artifact) = load_event_only_artifact() else {
        eprintln!("skipping: event_only_contract_compiled.json artifact not available");
        return;
    };

    let Some((wallet, default_account)) = setup_wallet(TEST_ACCOUNT_0).await else {
        return;
    };

    // Deploy the EventOnly contract
    let (contract_address, artifact, _instance) =
        deploy_contract(&wallet, artifact, vec![], default_account).await;

    // Emit an event with a random value
    let value = Fr::random();
    let tx_result = wallet
        .send_tx(
            ExecutionPayload {
                calls: vec![build_call(
                    &artifact,
                    contract_address,
                    "emit_event_for_msg_sender",
                    vec![AbiValue::Field(value)],
                )],
                ..Default::default()
            },
            SendOptions {
                from: default_account,
                ..Default::default()
            },
        )
        .await
        .expect("send emit_event_for_msg_sender tx");

    let receipt = wallet
        .node()
        .get_tx_receipt(&tx_result.tx_hash)
        .await
        .expect("get tx receipt");
    let block_number = receipt.block_number.expect("block number present");

    // Retrieve the private event and verify its value
    let events = wallet
        .get_private_events(
            &test_event_metadata(),
            PrivateEventFilter {
                contract_address,
                from_block: Some(block_number),
                to_block: Some(block_number + 1),
                scopes: vec![default_account],
                ..Default::default()
            },
        )
        .await
        .expect("get private TestEvent");

    assert_eq!(events.len(), 1, "expected exactly one TestEvent");

    let obj = events[0].event.as_object().expect("event is object");
    let value_str = obj["value"].as_str().expect("value as str");
    let decoded_value = Fr::from_hex(value_str).expect("parse value");
    assert_eq!(decoded_value, value, "TestEvent.value should match emitted");
}
