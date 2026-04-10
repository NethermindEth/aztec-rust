//! Event service for validating and storing private events.
//!
//! Ports the TS `EventService` which validates that events exist in
//! transaction effects (via siloed event commitment as nullifier)
//! and stores them in the PrivateEventStore.

use aztec_core::error::Error;
use aztec_core::tx::TxHash;
use aztec_core::types::{AztecAddress, Fr};
use aztec_node_client::AztecNode;

use crate::stores::private_event_store::{PrivateEventQueryFilter, StoredPrivateEvent};
use crate::stores::{AnchorBlockStore, PrivateEventStore};

use aztec_core::abi::EventSelector;

/// Service for validating and storing private events.
///
/// Matches the TS `EventService` — validates that events exist in the
/// transaction's nullifier set (via siloed event commitment) and stores
/// them in the PrivateEventStore with proper metadata.
pub struct EventService<'a, N: AztecNode> {
    node: &'a N,
    private_event_store: &'a PrivateEventStore,
    anchor_block_store: &'a AnchorBlockStore,
}

impl<'a, N: AztecNode> EventService<'a, N> {
    pub fn new(
        node: &'a N,
        private_event_store: &'a PrivateEventStore,
        anchor_block_store: &'a AnchorBlockStore,
    ) -> Self {
        Self {
            node,
            private_event_store,
            anchor_block_store,
        }
    }

    /// Validate and store a private event.
    ///
    /// Validates:
    /// 1. The tx effect exists and is at or before the anchor block
    /// 2. The siloed event commitment is present as a nullifier in the tx
    ///
    /// Then stores the event in the PrivateEventStore with full metadata
    /// (block hash, tx index in block, event index in tx).
    pub async fn validate_and_store_event(
        &self,
        contract_address: &AztecAddress,
        selector: &EventSelector,
        randomness: Fr,
        content: Vec<Fr>,
        event_commitment: Fr,
        tx_hash: TxHash,
        scope: &AztecAddress,
    ) -> Result<(), Error> {
        // Compute the siloed event commitment
        let siloed_event_commitment = aztec_core::hash::poseidon2_hash_with_separator(
            &[Fr::from(*contract_address), event_commitment],
            0,
        );

        // Get the anchor block number to verify the tx is within range
        let anchor_block_number = self.anchor_block_store.get_block_number().await?;

        // Get the tx effect to validate inclusion and extract metadata.
        // Upstream uses getTxEffect() to verify the siloed event commitment
        // is present in the tx nullifiers and to extract positional metadata.
        let tx_effect =
            self.node.get_tx_effect(&tx_hash).await?.ok_or_else(|| {
                Error::InvalidData(format!("tx effect not found for tx {tx_hash}"))
            })?;

        // Extract block number from the tx effect
        let block_number = tx_effect
            .pointer("/l2BlockNumber")
            .or_else(|| tx_effect.get("l2BlockNumber"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if block_number > anchor_block_number && anchor_block_number > 0 {
            return Err(Error::InvalidData(format!(
                "tx {} is in block {block_number} which is after anchor block {anchor_block_number}",
                tx_hash
            )));
        }

        // Extract block hash
        let l2_block_hash = tx_effect
            .pointer("/l2BlockHash")
            .or_else(|| tx_effect.get("l2BlockHash"))
            .and_then(|v| v.as_str())
            .unwrap_or("0x0")
            .to_owned();

        // Extract positional metadata
        let tx_index_in_block = tx_effect
            .pointer("/txIndexInBlock")
            .or_else(|| tx_effect.get("txIndexInBlock"))
            .and_then(|v| v.as_u64());

        // Validate the siloed event commitment is present as a nullifier
        let nullifiers = tx_effect
            .pointer("/nullifiers")
            .or_else(|| tx_effect.get("nullifiers"))
            .and_then(|v| v.as_array());

        if let Some(nullifiers) = nullifiers {
            let commitment_hex = format!("{siloed_event_commitment}");
            let found = nullifiers
                .iter()
                .any(|n| n.as_str().map_or(false, |s| s == commitment_hex));
            if !found {
                return Err(Error::InvalidData(format!(
                    "siloed event commitment {commitment_hex} not found in tx {tx_hash} nullifiers"
                )));
            }
        }

        // Derive event index from nullifier position
        let event_index_in_tx = nullifiers.and_then(|nullifiers| {
            let commitment_hex = format!("{siloed_event_commitment}");
            nullifiers
                .iter()
                .position(|n| n.as_str().map_or(false, |s| s == commitment_hex))
                .map(|i| i as u64)
        });

        // Store the event
        let event = StoredPrivateEvent {
            event_selector: *selector,
            randomness,
            msg_content: content,
            siloed_event_commitment,
            contract_address: *contract_address,
            scopes: vec![],
            tx_hash,
            l2_block_number: block_number,
            l2_block_hash,
            tx_index_in_block,
            event_index_in_tx,
        };

        self.private_event_store
            .store_private_event_log(&event, scope)
            .await?;

        tracing::debug!(
            contract = %contract_address,
            event_selector = %selector.0,
            block = block_number,
            "stored private event"
        );

        Ok(())
    }

    /// Get private events for a contract and event selector.
    pub async fn get_private_events(
        &self,
        event_selector: &EventSelector,
        filter: &PrivateEventQueryFilter,
    ) -> Result<Vec<StoredPrivateEvent>, Error> {
        self.private_event_store
            .get_private_events(event_selector, filter)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::InMemoryKvStore;
    use std::sync::Arc;

    // Minimal mock node for event service tests
    struct MockNode;

    #[async_trait::async_trait]
    impl AztecNode for MockNode {
        async fn get_node_info(&self) -> Result<aztec_node_client::NodeInfo, Error> {
            Ok(aztec_node_client::NodeInfo {
                node_version: "mock".into(),
                l1_chain_id: 1,
                rollup_version: 1,
                enr: None,
                l1_contract_addresses: serde_json::Value::Null,
                protocol_contract_addresses: serde_json::Value::Null,
                real_proofs: false,
                l2_circuits_vk_tree_root: None,
                l2_protocol_contracts_hash: None,
            })
        }
        async fn get_block_number(&self) -> Result<u64, Error> {
            Ok(5)
        }
        async fn get_proven_block_number(&self) -> Result<u64, Error> {
            Ok(5)
        }
        async fn get_tx_receipt(
            &self,
            _tx_hash: &TxHash,
        ) -> Result<aztec_core::tx::TxReceipt, Error> {
            Ok(aztec_core::tx::TxReceipt {
                tx_hash: TxHash::zero(),
                status: aztec_core::tx::TxStatus::Proposed,
                execution_result: Some(aztec_core::tx::TxExecutionResult::Success),
                error: None,
                transaction_fee: None,
                block_hash: None,
                block_number: Some(3),
                epoch_number: None,
            })
        }
        async fn get_tx_effect(
            &self,
            _tx_hash: &TxHash,
        ) -> Result<Option<serde_json::Value>, Error> {
            // Return a tx effect with block metadata.
            // Nullifiers field is omitted so validation is skipped in this basic mock.
            // Tests that need nullifier validation should use a richer mock.
            Ok(Some(serde_json::json!({
                "l2BlockNumber": 3,
                "l2BlockHash": "0x0000000000000000000000000000000000000000000000000000000000000003",
                "txIndexInBlock": 0
            })))
        }
        async fn get_tx_by_hash(
            &self,
            _tx_hash: &TxHash,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_public_logs(
            &self,
            _: aztec_node_client::PublicLogFilter,
        ) -> Result<aztec_node_client::PublicLogsResponse, Error> {
            Ok(aztec_node_client::PublicLogsResponse {
                logs: vec![],
                max_logs_hit: false,
            })
        }
        async fn send_tx(&self, _: &serde_json::Value) -> Result<(), Error> {
            Ok(())
        }
        async fn get_contract(
            &self,
            _: &AztecAddress,
        ) -> Result<Option<aztec_core::types::ContractInstanceWithAddress>, Error> {
            Ok(None)
        }
        async fn get_contract_class(&self, _: &Fr) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_block_header(&self, _: u64) -> Result<serde_json::Value, Error> {
            Ok(serde_json::json!({}))
        }
        async fn get_block(&self, _: u64) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_note_hash_membership_witness(
            &self,
            _: u64,
            _: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_nullifier_membership_witness(
            &self,
            _: u64,
            _: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_low_nullifier_membership_witness(
            &self,
            _: u64,
            _: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_public_storage_at(
            &self,
            _: u64,
            _: &AztecAddress,
            _: &Fr,
        ) -> Result<Fr, Error> {
            Ok(Fr::zero())
        }
        async fn get_public_data_witness(
            &self,
            _: u64,
            _: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_l1_to_l2_message_membership_witness(
            &self,
            _: u64,
            _: &Fr,
        ) -> Result<serde_json::Value, Error> {
            Ok(serde_json::Value::Null)
        }
        async fn simulate_public_calls(
            &self,
            _: &serde_json::Value,
            _: bool,
        ) -> Result<serde_json::Value, Error> {
            Ok(serde_json::Value::Null)
        }
        async fn is_valid_tx(
            &self,
            _: &serde_json::Value,
        ) -> Result<aztec_node_client::TxValidationResult, Error> {
            Ok(aztec_node_client::TxValidationResult::Valid)
        }
        async fn get_private_logs_by_tags(&self, _: &[Fr]) -> Result<serde_json::Value, Error> {
            Ok(serde_json::json!([]))
        }
        async fn get_public_logs_by_tags_from_contract(
            &self,
            _: &AztecAddress,
            _: &[Fr],
        ) -> Result<serde_json::Value, Error> {
            Ok(serde_json::json!([]))
        }
        async fn register_contract_function_signatures(&self, _: &[String]) -> Result<(), Error> {
            Ok(())
        }
        async fn get_block_hash_membership_witness(
            &self,
            _: u64,
            _: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn find_leaves_indexes(
            &self,
            _: u64,
            _: &str,
            _: &[Fr],
        ) -> Result<Vec<Option<u64>>, Error> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn validate_and_store_event_stores_correctly() {
        let kv: Arc<dyn crate::stores::kv::KvStore> = Arc::new(InMemoryKvStore::new());
        let event_store = PrivateEventStore::new(Arc::clone(&kv));
        let anchor_store = AnchorBlockStore::new(Arc::clone(&kv));

        // Set anchor to block 5
        let anchor = crate::stores::anchor_block_store::AnchorBlockHeader::from_header_json(
            serde_json::json!({"globalVariables": {"blockNumber": 5}}),
        );
        anchor_store.set_header(&anchor).await.unwrap();

        let node = MockNode;
        let service = EventService::new(&node, &event_store, &anchor_store);

        let contract = AztecAddress::from(1u64);
        let selector = EventSelector(Fr::from(0x12345678u64));
        let scope = AztecAddress::from(99u64);
        let tx_hash = TxHash::zero();

        service
            .validate_and_store_event(
                &contract,
                &selector,
                Fr::from(1u64),
                vec![Fr::from(10u64)],
                Fr::from(100u64),
                tx_hash,
                &scope,
            )
            .await
            .unwrap();

        let events = event_store
            .get_private_events(
                &selector,
                &PrivateEventQueryFilter {
                    contract_address: contract,
                    from_block: None,
                    to_block: None,
                    scopes: vec![scope],
                    tx_hash: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].l2_block_number, 3);
    }
}
