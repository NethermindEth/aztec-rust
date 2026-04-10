use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::abi::AbiType;
use crate::error::Error;
use crate::node::{AztecNode, LogId, PublicLogFilter};
use crate::tx::TxHash;
use crate::types::{AztecAddress, Fr};
use crate::wallet::EventMetadataDefinition;

// ---------------------------------------------------------------------------
// Public event types
// ---------------------------------------------------------------------------

/// Metadata attached to a decoded public event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicEventMetadata {
    /// Address of the contract that emitted the event.
    pub contract_address: AztecAddress,
    /// Hash of the transaction that emitted the event.
    pub tx_hash: Option<TxHash>,
    /// Block number containing the event.
    pub block_number: u64,
    /// Log index within the block.
    pub log_index: u64,
}

/// A decoded public event with metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicEvent<T> {
    /// Decoded event data.
    pub event: T,
    /// Event metadata (contract, block, index).
    pub metadata: PublicEventMetadata,
}

/// Filter for querying public events from the node.
///
/// This wraps the underlying [`PublicLogFilter`] but omits the `selector`
/// field which is provided automatically from the [`EventMetadataDefinition`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicEventFilter {
    /// Filter by transaction hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<TxHash>,
    /// Start block (inclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_block: Option<u64>,
    /// End block (inclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_block: Option<u64>,
    /// Filter by emitting contract address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_address: Option<AztecAddress>,
    /// Cursor for pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_log: Option<LogId>,
}

/// Result of a public events query.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPublicEventsResult<T> {
    /// Decoded events.
    pub events: Vec<PublicEvent<T>>,
    /// Whether the response was truncated due to the log limit.
    pub max_logs_hit: bool,
}

// ---------------------------------------------------------------------------
// Decoding helpers
// ---------------------------------------------------------------------------

/// Decode a single public log's field-element data into named fields.
///
/// Noir's `emit_public_log` places the event selector at the **last** position:
/// `[serialized_event_fields..., event_type_id]`.
/// The field elements before the selector are mapped positionally to field names.
fn decode_log_fields(
    data: &[Fr],
    event_metadata: &EventMetadataDefinition,
) -> Result<BTreeMap<String, Fr>, Error> {
    if data.is_empty() {
        return Err(Error::Abi("log data is empty".into()));
    }

    // Event selector is the last field element.
    let selector = *data.last().expect("non-empty");
    if selector != event_metadata.event_selector.0 {
        return Err(Error::Abi(format!(
            "event selector mismatch: expected {}, got {}",
            event_metadata.event_selector.0, selector
        )));
    }

    // Data fields are everything before the selector.
    let field_data = &data[..data.len() - 1];
    let names: Vec<String> = match &event_metadata.abi_type {
        AbiType::Struct { fields, .. } => {
            if event_metadata.field_names.is_empty() {
                fields.iter().map(|field| field.name.clone()).collect()
            } else {
                if event_metadata.field_names.len() != fields.len() {
                    return Err(Error::Abi(format!(
                        "event metadata field name count {} does not match abi struct field count {}",
                        event_metadata.field_names.len(),
                        fields.len()
                    )));
                }
                event_metadata.field_names.clone()
            }
        }
        _ => event_metadata.field_names.clone(),
    };

    if field_data.len() < names.len() {
        return Err(Error::Abi(format!(
            "not enough fields in log data: expected at least {}, got {}",
            names.len(),
            field_data.len()
        )));
    }

    let mut result = BTreeMap::new();
    for (i, name) in names.iter().enumerate() {
        result.insert(name.clone(), field_data[i]);
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Public event query
// ---------------------------------------------------------------------------

/// Query and decode public events from the node.
///
/// Fetches all public logs matching the filter, then performs **client-side**
/// filtering by event selector (matching the last field of each log against
/// the selector from `event_metadata`).  This mirrors the upstream TS SDK
/// behaviour where the node returns all logs and the client skips non-matching
/// selectors.
pub async fn get_public_events(
    node: &(impl AztecNode + ?Sized),
    event_metadata: &EventMetadataDefinition,
    filter: PublicEventFilter,
) -> Result<GetPublicEventsResult<BTreeMap<String, Fr>>, Error> {
    let log_filter = PublicLogFilter {
        tx_hash: filter.tx_hash,
        from_block: filter.from_block,
        to_block: filter.to_block,
        contract_address: filter.contract_address,
        selector: None, // client-side filtering, not node-side
        after_log: filter.after_log,
    };

    let response = node.get_public_logs(log_filter).await?;

    let mut events = Vec::with_capacity(response.logs.len());
    for log in &response.logs {
        // Client-side selector matching: skip logs whose last field doesn't
        // match the expected event selector.
        if let Some(last) = log.data.last() {
            if *last != event_metadata.event_selector.0 {
                continue;
            }
        } else {
            continue;
        }
        let decoded = decode_log_fields(&log.data, event_metadata)?;
        events.push(PublicEvent {
            event: decoded,
            metadata: PublicEventMetadata {
                contract_address: log.contract_address,
                tx_hash: log.tx_hash,
                block_number: log.block_number,
                log_index: log.log_index,
            },
        });
    }

    Ok(GetPublicEventsResult {
        events,
        max_logs_hit: response.max_logs_hit,
    })
}

// ---------------------------------------------------------------------------
// Re-exports for convenience
// ---------------------------------------------------------------------------

// Private event types live in `wallet` but are re-exported here so that
// consumers can find all event-related types in one place.
pub use crate::wallet::{
    EventMetadataDefinition as EventMetadata, PrivateEvent, PrivateEventFilter,
    PrivateEventMetadata,
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::unimplemented)]
mod tests {
    use super::*;
    use crate::abi::{AbiParameter, EventSelector};
    use crate::node::{PublicLog, PublicLogsResponse};
    use async_trait::async_trait;
    use std::sync::Mutex;

    use crate::node::{AztecNode, NodeInfo, PublicLogFilter};
    use crate::tx::{TxHash, TxReceipt};

    // -- Mock node that returns configurable public logs --

    struct MockEventNode {
        logs_response: Mutex<PublicLogsResponse>,
        captured_filter: Mutex<Option<PublicLogFilter>>,
    }

    impl MockEventNode {
        fn new(response: PublicLogsResponse) -> Self {
            Self {
                logs_response: Mutex::new(response),
                captured_filter: Mutex::new(None),
            }
        }

        fn captured_filter(&self) -> Option<PublicLogFilter> {
            self.captured_filter.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl AztecNode for MockEventNode {
        async fn get_node_info(&self) -> Result<NodeInfo, Error> {
            unimplemented!("not needed for event tests")
        }

        async fn get_block_number(&self) -> Result<u64, Error> {
            unimplemented!("not needed for event tests")
        }

        async fn get_proven_block_number(&self) -> Result<u64, Error> {
            unimplemented!("not needed for event tests")
        }

        async fn get_tx_receipt(&self, _tx_hash: &TxHash) -> Result<TxReceipt, Error> {
            unimplemented!("not needed for event tests")
        }

        async fn get_tx_effect(
            &self,
            _tx_hash: &TxHash,
        ) -> Result<Option<serde_json::Value>, Error> {
            unimplemented!("not needed for event tests")
        }

        async fn get_public_logs(
            &self,
            filter: PublicLogFilter,
        ) -> Result<PublicLogsResponse, Error> {
            *self.captured_filter.lock().unwrap() = Some(filter);
            Ok(self.logs_response.lock().unwrap().clone())
        }

        async fn send_tx(&self, _tx: &serde_json::Value) -> Result<(), Error> {
            unimplemented!("not needed for event tests")
        }

        async fn get_contract(
            &self,
            _address: &AztecAddress,
        ) -> Result<Option<aztec_core::types::ContractInstanceWithAddress>, Error> {
            unimplemented!("not needed for event tests")
        }

        async fn get_contract_class(&self, _id: &Fr) -> Result<Option<serde_json::Value>, Error> {
            unimplemented!("not needed for event tests")
        }

        async fn get_block_header(&self, _block_number: u64) -> Result<serde_json::Value, Error> {
            unimplemented!("not needed for event tests")
        }
        async fn get_block(&self, _block_number: u64) -> Result<Option<serde_json::Value>, Error> {
            unimplemented!("not needed for event tests")
        }
        async fn get_note_hash_membership_witness(
            &self,
            _block_number: u64,
            _note_hash: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            unimplemented!("not needed for event tests")
        }
        async fn get_nullifier_membership_witness(
            &self,
            _block_number: u64,
            _nullifier: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            unimplemented!("not needed for event tests")
        }
        async fn get_low_nullifier_membership_witness(
            &self,
            _block_number: u64,
            _nullifier: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            unimplemented!("not needed for event tests")
        }
        async fn get_public_storage_at(
            &self,
            _block_number: u64,
            _contract: &AztecAddress,
            _slot: &Fr,
        ) -> Result<Fr, Error> {
            unimplemented!("not needed for event tests")
        }
        async fn get_public_data_witness(
            &self,
            _block_number: u64,
            _leaf_slot: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            unimplemented!("not needed for event tests")
        }
        async fn get_l1_to_l2_message_membership_witness(
            &self,
            _block_number: u64,
            _entry_key: &Fr,
        ) -> Result<serde_json::Value, Error> {
            unimplemented!("not needed for event tests")
        }
        async fn simulate_public_calls(
            &self,
            _tx: &serde_json::Value,
            _skip_fee_enforcement: bool,
        ) -> Result<serde_json::Value, Error> {
            unimplemented!("not needed for event tests")
        }
        async fn is_valid_tx(
            &self,
            _tx: &serde_json::Value,
        ) -> Result<aztec_node_client::TxValidationResult, Error> {
            unimplemented!("not needed for event tests")
        }
        async fn get_private_logs_by_tags(&self, _tags: &[Fr]) -> Result<serde_json::Value, Error> {
            unimplemented!("not needed for event tests")
        }
        async fn get_public_logs_by_tags_from_contract(
            &self,
            _contract: &AztecAddress,
            _tags: &[Fr],
        ) -> Result<serde_json::Value, Error> {
            unimplemented!("not needed for event tests")
        }
        async fn register_contract_function_signatures(
            &self,
            _signatures: &[String],
        ) -> Result<(), Error> {
            unimplemented!("not needed for event tests")
        }
        async fn get_block_hash_membership_witness(
            &self,
            _block_number: u64,
            _block_hash: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            unimplemented!("not needed for event tests")
        }
        async fn find_leaves_indexes(
            &self,
            _block_number: u64,
            _tree_id: &str,
            _leaves: &[Fr],
        ) -> Result<Vec<Option<u64>>, Error> {
            unimplemented!("not needed for event tests")
        }
        async fn get_tx_by_hash(
            &self,
            _tx_hash: &TxHash,
        ) -> Result<Option<serde_json::Value>, Error> {
            unimplemented!("not needed for event tests")
        }
    }

    fn sample_event_metadata() -> EventMetadataDefinition {
        EventMetadataDefinition {
            event_selector: EventSelector(Fr::from(42u64)),
            abi_type: AbiType::Struct {
                name: "Transfer".to_owned(),
                fields: vec![
                    AbiParameter {
                        name: "amount".to_owned(),
                        typ: AbiType::Field,
                        visibility: None,
                    },
                    AbiParameter {
                        name: "sender".to_owned(),
                        typ: AbiType::Field,
                        visibility: None,
                    },
                ],
            },
            field_names: vec!["amount".to_owned(), "sender".to_owned()],
        }
    }

    fn make_log(selector: Fr, fields: Vec<Fr>, block: u64, index: u64) -> PublicLog {
        let mut data = vec![selector];
        data.extend(fields);
        PublicLog {
            contract_address: AztecAddress(Fr::from(1u64)),
            data,
            tx_hash: Some(TxHash::zero()),
            block_number: block,
            log_index: index,
        }
    }

    // -- decode_log_fields tests --

    #[test]
    fn decode_log_fields_success() {
        let meta = sample_event_metadata();
        let data = vec![Fr::from(42u64), Fr::from(100u64), Fr::from(200u64)];

        let decoded = decode_log_fields(&data, &meta).expect("decode");
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded["amount"], Fr::from(100u64));
        assert_eq!(decoded["sender"], Fr::from(200u64));
    }

    #[test]
    fn decode_log_fields_extra_fields_ignored() {
        let meta = sample_event_metadata();
        let data = vec![
            Fr::from(42u64),
            Fr::from(100u64),
            Fr::from(200u64),
            Fr::from(300u64), // extra
        ];

        let decoded = decode_log_fields(&data, &meta).expect("decode");
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded["amount"], Fr::from(100u64));
        assert_eq!(decoded["sender"], Fr::from(200u64));
    }

    #[test]
    fn decode_log_fields_selector_mismatch() {
        let meta = sample_event_metadata();
        let data = vec![Fr::from(99u64), Fr::from(100u64), Fr::from(200u64)];

        let err = decode_log_fields(&data, &meta).unwrap_err();
        assert!(matches!(err, Error::Abi(_)));
        assert!(err.to_string().contains("selector mismatch"));
    }

    #[test]
    fn decode_log_fields_insufficient_fields() {
        let meta = sample_event_metadata();
        let data = vec![Fr::from(42u64), Fr::from(100u64)]; // only 1 field, need 2

        let err = decode_log_fields(&data, &meta).unwrap_err();
        assert!(matches!(err, Error::Abi(_)));
        assert!(err.to_string().contains("not enough fields"));
    }

    #[test]
    fn decode_log_fields_empty_data() {
        let meta = sample_event_metadata();
        let err = decode_log_fields(&[], &meta).unwrap_err();
        assert!(matches!(err, Error::Abi(_)));
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn decode_log_fields_no_field_names() {
        let meta = EventMetadataDefinition {
            event_selector: EventSelector(Fr::from(42u64)),
            abi_type: AbiType::Struct {
                name: "Transfer".to_owned(),
                fields: vec![
                    AbiParameter {
                        name: "amount".to_owned(),
                        typ: AbiType::Field,
                        visibility: None,
                    },
                    AbiParameter {
                        name: "sender".to_owned(),
                        typ: AbiType::Field,
                        visibility: None,
                    },
                ],
            },
            field_names: vec![],
        };
        let data = vec![Fr::from(42u64), Fr::from(100u64), Fr::from(200u64)];

        let decoded = decode_log_fields(&data, &meta).expect("decode");
        assert_eq!(decoded["amount"], Fr::from(100u64));
        assert_eq!(decoded["sender"], Fr::from(200u64));
    }

    #[test]
    fn decode_log_fields_mismatched_field_names_and_abi_fails() {
        let meta = EventMetadataDefinition {
            event_selector: EventSelector(Fr::from(42u64)),
            abi_type: AbiType::Struct {
                name: "Transfer".to_owned(),
                fields: vec![AbiParameter {
                    name: "amount".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                }],
            },
            field_names: vec!["amount".to_owned(), "sender".to_owned()],
        };
        let data = vec![Fr::from(42u64), Fr::from(100u64)];

        let err = decode_log_fields(&data, &meta).unwrap_err();
        assert!(matches!(err, Error::Abi(_)));
        assert!(err.to_string().contains("field name count"));
    }

    // -- get_public_events tests --

    #[tokio::test]
    async fn get_public_events_decodes_logs() {
        let meta = sample_event_metadata();
        let logs = vec![
            make_log(
                Fr::from(42u64),
                vec![Fr::from(100u64), Fr::from(200u64)],
                10,
                0,
            ),
            make_log(
                Fr::from(42u64),
                vec![Fr::from(300u64), Fr::from(400u64)],
                10,
                1,
            ),
        ];
        let node = MockEventNode::new(PublicLogsResponse {
            logs,
            max_logs_hit: false,
        });

        let result = get_public_events(&node, &meta, PublicEventFilter::default())
            .await
            .expect("get events");

        assert_eq!(result.events.len(), 2);
        assert!(!result.max_logs_hit);

        assert_eq!(result.events[0].event["amount"], Fr::from(100u64));
        assert_eq!(result.events[0].event["sender"], Fr::from(200u64));
        assert_eq!(result.events[0].metadata.block_number, 10);
        assert_eq!(result.events[0].metadata.log_index, 0);

        assert_eq!(result.events[1].event["amount"], Fr::from(300u64));
        assert_eq!(result.events[1].event["sender"], Fr::from(400u64));
        assert_eq!(result.events[1].metadata.log_index, 1);
    }

    #[tokio::test]
    async fn get_public_events_empty_response() {
        let meta = sample_event_metadata();
        let node = MockEventNode::new(PublicLogsResponse {
            logs: vec![],
            max_logs_hit: false,
        });

        let result = get_public_events(&node, &meta, PublicEventFilter::default())
            .await
            .expect("get events");

        assert!(result.events.is_empty());
        assert!(!result.max_logs_hit);
    }

    #[tokio::test]
    async fn get_public_events_propagates_max_logs_hit() {
        let meta = sample_event_metadata();
        let node = MockEventNode::new(PublicLogsResponse {
            logs: vec![make_log(
                Fr::from(42u64),
                vec![Fr::from(1u64), Fr::from(2u64)],
                5,
                0,
            )],
            max_logs_hit: true,
        });

        let result = get_public_events(&node, &meta, PublicEventFilter::default())
            .await
            .expect("get events");

        assert_eq!(result.events.len(), 1);
        assert!(result.max_logs_hit);
    }

    #[tokio::test]
    async fn get_public_events_passes_filter_with_selector() {
        let meta = sample_event_metadata();
        let contract = AztecAddress(Fr::from(99u64));
        let after = LogId {
            block_number: 5,
            log_index: 3,
        };
        let tx_hash =
            TxHash::from_hex("0x0000000000000000000000000000000000000000000000000000000000000001")
                .expect("valid tx hash");

        let node = MockEventNode::new(PublicLogsResponse {
            logs: vec![],
            max_logs_hit: false,
        });

        let filter = PublicEventFilter {
            tx_hash: Some(tx_hash),
            from_block: Some(1),
            to_block: Some(100),
            contract_address: Some(contract),
            after_log: Some(after.clone()),
        };

        let _result = get_public_events(&node, &meta, filter)
            .await
            .expect("get events");

        let captured = node.captured_filter().expect("filter was captured");
        assert_eq!(captured.tx_hash, Some(tx_hash));
        assert_eq!(captured.from_block, Some(1));
        assert_eq!(captured.to_block, Some(100));
        assert_eq!(captured.contract_address, Some(contract));
        assert_eq!(captured.after_log, Some(after));
        assert_eq!(captured.selector, Some(EventSelector(Fr::from(42u64))));
    }

    #[tokio::test]
    async fn get_public_events_decode_error_propagates() {
        let meta = sample_event_metadata();
        // Log with wrong selector
        let logs = vec![make_log(
            Fr::from(99u64),
            vec![Fr::from(1u64), Fr::from(2u64)],
            1,
            0,
        )];
        let node = MockEventNode::new(PublicLogsResponse {
            logs,
            max_logs_hit: false,
        });

        let err = get_public_events(&node, &meta, PublicEventFilter::default())
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Abi(_)));
    }

    // -- PublicEventFilter serde --

    #[test]
    fn public_event_filter_default_serializes_empty() {
        let filter = PublicEventFilter::default();
        let json = serde_json::to_value(&filter).unwrap();
        assert_eq!(json, serde_json::json!({}));
    }

    #[test]
    fn public_event_filter_with_fields() {
        let tx_hash =
            TxHash::from_hex("0x0000000000000000000000000000000000000000000000000000000000000002")
                .expect("valid tx hash");
        let filter = PublicEventFilter {
            tx_hash: Some(tx_hash),
            from_block: Some(10),
            to_block: Some(20),
            ..Default::default()
        };
        let json = serde_json::to_value(&filter).unwrap();
        assert_eq!(json["txHash"], tx_hash.to_string());
        assert_eq!(json["fromBlock"], 10);
        assert_eq!(json["toBlock"], 20);
        assert!(json.get("contractAddress").is_none());
        assert!(json.get("afterLog").is_none());
    }

    // -- PublicEventMetadata serde --

    #[test]
    fn public_event_metadata_roundtrip() {
        let meta = PublicEventMetadata {
            contract_address: AztecAddress(Fr::from(1u64)),
            tx_hash: Some(TxHash::zero()),
            block_number: 42,
            log_index: 7,
        };
        let json = serde_json::to_string(&meta).expect("serialize");
        let decoded: PublicEventMetadata = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, meta);
    }

    // -- PublicEvent serde --

    #[test]
    fn public_event_roundtrip() {
        let mut fields = BTreeMap::new();
        fields.insert("amount".to_owned(), Fr::from(100u64));
        let event = PublicEvent {
            event: fields,
            metadata: PublicEventMetadata {
                contract_address: AztecAddress(Fr::from(1u64)),
                tx_hash: Some(TxHash::zero()),
                block_number: 10,
                log_index: 0,
            },
        };
        let json = serde_json::to_string(&event).expect("serialize");
        let decoded: PublicEvent<BTreeMap<String, Fr>> =
            serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.event["amount"], Fr::from(100u64));
        assert_eq!(decoded.metadata.block_number, 10);
    }

    // -- GetPublicEventsResult serde --

    #[test]
    fn get_public_events_result_roundtrip() {
        let result: GetPublicEventsResult<BTreeMap<String, Fr>> = GetPublicEventsResult {
            events: vec![],
            max_logs_hit: false,
        };
        let json = serde_json::to_string(&result).expect("serialize");
        let decoded: GetPublicEventsResult<BTreeMap<String, Fr>> =
            serde_json::from_str(&json).expect("deserialize");
        assert!(decoded.events.is_empty());
        assert!(!decoded.max_logs_hit);
    }
}
