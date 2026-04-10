//! Log service for tagged log retrieval and storage.
//!
//! Ports the TS `LogService` which manages log retrieval via the tagging
//! protocol, supporting both public and private logs with pagination.

use aztec_core::error::Error;
use aztec_core::hash::{compute_siloed_private_log_first_field, poseidon2_hash};
use aztec_core::tx::TxHash;
use aztec_core::types::{AztecAddress, Fr};
use aztec_node_client::AztecNode;

use crate::stores::{CapsuleStore, RecipientTaggingStore, SenderStore, SenderTaggingStore};

/// Maximum number of tags per RPC request.
const MAX_RPC_LEN: usize = 128;

/// Window length for unfinalized tagging indexes.
const UNFINALIZED_TAGGING_INDEXES_WINDOW_LEN: u64 = 20;

/// A request to retrieve logs by tag.
#[derive(Debug, Clone)]
pub struct LogRetrievalRequest {
    /// Whether the log is public or private.
    pub is_public: bool,
    /// The tag to search for.
    pub tag: Fr,
    /// The contract address (for public logs).
    pub contract_address: Option<AztecAddress>,
}

/// A retrieved tagged log entry.
#[derive(Debug, Clone)]
pub struct TaggedLog {
    /// The tag that matched.
    pub tag: Fr,
    /// The log data fields.
    pub data: Vec<Fr>,
    /// Block number containing the log.
    pub block_number: u64,
    /// Whether this is a public log.
    pub is_public: bool,
    /// Transaction hash that emitted the log.
    pub tx_hash: TxHash,
    /// Unique note hashes created by the emitting transaction.
    pub note_hashes: Vec<Fr>,
    /// First nullifier created by the emitting transaction.
    pub first_nullifier: Fr,
}

/// Service for log retrieval operations using the tagging protocol.
pub struct LogService<'a, N: AztecNode> {
    node: &'a N,
    sender_store: &'a SenderStore,
    #[allow(dead_code)] // Used when sender-side tag sync is wired
    sender_tagging_store: &'a SenderTaggingStore,
    recipient_tagging_store: &'a RecipientTaggingStore,
    #[allow(dead_code)]
    capsule_store: &'a CapsuleStore,
}

impl<'a, N: AztecNode> LogService<'a, N> {
    pub fn new(
        node: &'a N,
        sender_store: &'a SenderStore,
        sender_tagging_store: &'a SenderTaggingStore,
        recipient_tagging_store: &'a RecipientTaggingStore,
        capsule_store: &'a CapsuleStore,
    ) -> Self {
        Self {
            node,
            sender_store,
            sender_tagging_store,
            recipient_tagging_store,
            capsule_store,
        }
    }

    /// Bulk retrieve logs by tags.
    ///
    /// Fetches both public and private logs for multiple tag requests,
    /// handling pagination automatically.
    pub async fn bulk_retrieve_logs(
        &self,
        requests: &[LogRetrievalRequest],
    ) -> Result<Vec<Vec<TaggedLog>>, Error> {
        let mut results = Vec::with_capacity(requests.len());
        for request in requests {
            let public_logs = if let Some(contract) = &request.contract_address {
                self.get_public_logs_by_tag(contract, &request.tag).await?
            } else {
                vec![]
            };
            // The request tag from Noir is UNSILOED.  The node indexes
            // private logs by the SILOED first field, so we must silo
            // before querying.
            let siloed_tag = if let Some(contract) = &request.contract_address {
                compute_siloed_private_log_first_field(contract, &request.tag)
            } else {
                request.tag
            };
            let mut private_logs = self.get_private_logs_by_tags(&[siloed_tag]).await?;
            let private_logs = private_logs.pop().unwrap_or_default();

            if !public_logs.is_empty() && !private_logs.is_empty() {
                return Err(Error::InvalidData(format!(
                    "found both a public and private log for tag {}",
                    request.tag
                )));
            }

            results.push(if !public_logs.is_empty() {
                public_logs
            } else {
                private_logs
            });
        }

        Ok(results)
    }

    /// Fetch all tagged logs for a contract, handling multiple recipients and senders.
    ///
    /// This is the main entry point for note discovery via the tagging protocol.
    pub async fn fetch_tagged_logs(
        &self,
        contract_address: &AztecAddress,
        recipient: &AztecAddress,
        tagging_secrets: &[Fr],
    ) -> Result<Vec<TaggedLog>, Error> {
        let mut all_logs = Vec::new();

        for secret in tagging_secrets {
            let finalized = self
                .recipient_tagging_store
                .get_highest_finalized_index(secret)
                .await?;

            // Load logs for index range: (finalized, finalized + WINDOW_LEN]
            let from_index = finalized + 1;
            let to_index = finalized + UNFINALIZED_TAGGING_INDEXES_WINDOW_LEN;

            let logs = self
                .load_logs_for_range(contract_address, secret, from_index, to_index)
                .await?;

            all_logs.extend(logs);
        }

        if !all_logs.is_empty() {
            tracing::debug!(
                contract = %contract_address,
                recipient = %recipient,
                count = all_logs.len(),
                "fetched tagged logs"
            );
        }

        Ok(all_logs)
    }

    /// Load logs for a range of tagging indexes.
    async fn load_logs_for_range(
        &self,
        contract_address: &AztecAddress,
        secret: &Fr,
        from_index: u64,
        to_index: u64,
    ) -> Result<Vec<TaggedLog>, Error> {
        // Compute siloed tags for each index in range
        let mut tags = Vec::new();
        for idx in from_index..=to_index {
            let tag = compute_siloed_tag(secret, idx, contract_address);
            tags.push(tag);
        }

        // Fetch logs in batches
        let mut all_logs = Vec::new();
        for (chunk_idx, chunk) in tags.chunks(MAX_RPC_LEN).enumerate() {
            let logs = self.get_private_logs_by_tags(chunk).await?;
            let mut highest_found_index = None;
            for (i, tag_logs) in logs.into_iter().enumerate() {
                let idx = from_index + (chunk_idx * MAX_RPC_LEN + i) as u64;
                let had_logs = !tag_logs.is_empty();
                for log in tag_logs {
                    all_logs.push(log);
                }

                if had_logs {
                    highest_found_index = Some(idx);
                }
            }

            if let Some(idx) = highest_found_index {
                self.recipient_tagging_store
                    .update_highest_finalized_index(secret, idx)
                    .await
                    .ok();
            }
        }

        Ok(all_logs)
    }

    /// Fetch private logs by siloed tags from the node.
    async fn get_private_logs_by_tags(&self, tags: &[Fr]) -> Result<Vec<Vec<TaggedLog>>, Error> {
        if tags.is_empty() {
            return Ok(vec![]);
        }

        let response = self.node.get_private_logs_by_tags(tags).await?;

        // Parse the response — each tag gets an array of log entries
        let mut results = vec![Vec::new(); tags.len()];
        if let Some(outer) = response.as_array() {
            for (tag_idx, tag_logs) in outer.iter().enumerate().take(tags.len()) {
                let mut logs = Vec::new();
                if let Some(entries) = tag_logs.as_array() {
                    for entry in entries {
                        let data = entry
                            .get("logData")
                            .or_else(|| entry.get("data"))
                            .and_then(|d| d.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().and_then(|s| Fr::from_hex(s).ok()))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();

                        let block_number = entry
                            .get("blockNumber")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let tx_hash = entry
                            .get("txHash")
                            .and_then(|v| v.as_str())
                            .map(TxHash::from_hex)
                            .transpose()?
                            .unwrap_or_else(TxHash::zero);
                        let note_hashes = entry
                            .get("noteHashes")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().and_then(|s| Fr::from_hex(s).ok()))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let first_nullifier = entry
                            .get("firstNullifier")
                            .and_then(|v| v.as_str())
                            .map(Fr::from_hex)
                            .transpose()?
                            .unwrap_or_else(Fr::zero);

                        logs.push(TaggedLog {
                            tag: tags[tag_idx],
                            data,
                            block_number,
                            is_public: false,
                            tx_hash,
                            note_hashes,
                            first_nullifier,
                        });
                    }
                }
                results[tag_idx] = logs;
            }
        }

        Ok(results)
    }

    /// Fetch public logs by tag from a specific contract.
    async fn get_public_logs_by_tag(
        &self,
        contract: &AztecAddress,
        tag: &Fr,
    ) -> Result<Vec<TaggedLog>, Error> {
        let response = self
            .node
            .get_public_logs_by_tags_from_contract(contract, &[*tag])
            .await?;

        let mut logs = Vec::new();
        if let Some(outer) = response.as_array() {
            for tag_logs in outer {
                if let Some(entries) = tag_logs.as_array() {
                    for entry in entries {
                        let data = entry
                            .get("logData")
                            .or_else(|| entry.get("data"))
                            .and_then(|d| d.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().and_then(|s| Fr::from_hex(s).ok()))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();

                        let block_number = entry
                            .get("blockNumber")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let tx_hash = entry
                            .get("txHash")
                            .and_then(|v| v.as_str())
                            .map(TxHash::from_hex)
                            .transpose()?
                            .unwrap_or_else(TxHash::zero);
                        let note_hashes = entry
                            .get("noteHashes")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().and_then(|s| Fr::from_hex(s).ok()))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let first_nullifier = entry
                            .get("firstNullifier")
                            .and_then(|v| v.as_str())
                            .map(Fr::from_hex)
                            .transpose()?
                            .unwrap_or_else(Fr::zero);

                        logs.push(TaggedLog {
                            tag: *tag,
                            data,
                            block_number,
                            is_public: true,
                            tx_hash,
                            note_hashes,
                            first_nullifier,
                        });
                    }
                }
            }
        }

        Ok(logs)
    }

    /// Get all registered senders.
    pub async fn get_senders(&self) -> Result<Vec<AztecAddress>, Error> {
        self.sender_store.get_all().await
    }
}

/// Compute a siloed tag from a secret and index.
///
/// In the full implementation, this uses ExtendedDirectionalAppTaggingSecret
/// and Poseidon2 hashing. For now, use a simple derivation with a separator.
fn compute_siloed_tag(secret: &Fr, index: u64, contract_address: &AztecAddress) -> Fr {
    let tag = poseidon2_hash(&[*secret, Fr::from(index)]);
    compute_siloed_private_log_first_field(contract_address, &tag)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::stores::{
        kv::KvStore, CapsuleStore, InMemoryKvStore, RecipientTaggingStore, SenderStore,
        SenderTaggingStore,
    };
    use aztec_core::tx::{TxExecutionResult, TxHash, TxReceipt, TxStatus};
    use aztec_core::types::{ContractInstanceWithAddress, Fr};
    use aztec_node_client::{NodeInfo, PublicLogFilter, PublicLogsResponse};

    struct MockNode {
        private_logs: serde_json::Value,
    }

    #[async_trait::async_trait]
    impl AztecNode for MockNode {
        async fn get_node_info(&self) -> Result<NodeInfo, Error> {
            Ok(NodeInfo {
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
            Ok(1)
        }
        async fn get_proven_block_number(&self) -> Result<u64, Error> {
            Ok(1)
        }
        async fn get_tx_receipt(&self, _tx_hash: &TxHash) -> Result<TxReceipt, Error> {
            Ok(TxReceipt {
                tx_hash: TxHash::zero(),
                status: TxStatus::Pending,
                execution_result: Some(TxExecutionResult::Success),
                error: None,
                transaction_fee: None,
                block_hash: None,
                block_number: None,
                epoch_number: None,
            })
        }
        async fn get_tx_effect(
            &self,
            _tx_hash: &TxHash,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_tx_by_hash(
            &self,
            _tx_hash: &TxHash,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_public_logs(
            &self,
            _filter: PublicLogFilter,
        ) -> Result<PublicLogsResponse, Error> {
            Ok(PublicLogsResponse {
                logs: vec![],
                max_logs_hit: false,
            })
        }
        async fn send_tx(&self, _tx: &serde_json::Value) -> Result<(), Error> {
            Ok(())
        }
        async fn get_contract(
            &self,
            _address: &AztecAddress,
        ) -> Result<Option<ContractInstanceWithAddress>, Error> {
            Ok(None)
        }
        async fn get_contract_class(&self, _id: &Fr) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_block_header(&self, _block_number: u64) -> Result<serde_json::Value, Error> {
            Ok(serde_json::json!({}))
        }
        async fn get_block(&self, _block_number: u64) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_note_hash_membership_witness(
            &self,
            _block_number: u64,
            _note_hash: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_nullifier_membership_witness(
            &self,
            _block_number: u64,
            _nullifier: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_low_nullifier_membership_witness(
            &self,
            _block_number: u64,
            _nullifier: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_public_storage_at(
            &self,
            _block_number: u64,
            _contract: &AztecAddress,
            _slot: &Fr,
        ) -> Result<Fr, Error> {
            Ok(Fr::zero())
        }
        async fn get_public_data_witness(
            &self,
            _block_number: u64,
            _leaf_slot: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_l1_to_l2_message_membership_witness(
            &self,
            _block_number: u64,
            _entry_key: &Fr,
        ) -> Result<serde_json::Value, Error> {
            Ok(serde_json::Value::Null)
        }
        async fn simulate_public_calls(
            &self,
            _tx: &serde_json::Value,
            _skip_fee_enforcement: bool,
        ) -> Result<serde_json::Value, Error> {
            Ok(serde_json::Value::Null)
        }
        async fn is_valid_tx(
            &self,
            _tx: &serde_json::Value,
        ) -> Result<aztec_node_client::TxValidationResult, Error> {
            Ok(aztec_node_client::TxValidationResult::Valid)
        }
        async fn get_private_logs_by_tags(&self, _tags: &[Fr]) -> Result<serde_json::Value, Error> {
            Ok(self.private_logs.clone())
        }
        async fn get_public_logs_by_tags_from_contract(
            &self,
            _contract: &AztecAddress,
            _tags: &[Fr],
        ) -> Result<serde_json::Value, Error> {
            Ok(serde_json::json!([]))
        }
        async fn register_contract_function_signatures(
            &self,
            _signatures: &[String],
        ) -> Result<(), Error> {
            Ok(())
        }
        async fn get_block_hash_membership_witness(
            &self,
            _block_number: u64,
            _block_hash: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn find_leaves_indexes(
            &self,
            _block_number: u64,
            _tree_id: &str,
            _leaves: &[Fr],
        ) -> Result<Vec<Option<u64>>, Error> {
            Ok(vec![])
        }
    }

    fn make_service(
        private_logs: serde_json::Value,
    ) -> (
        LogService<'static, MockNode>,
        &'static RecipientTaggingStore,
        Fr,
    ) {
        let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::new());
        let secret = Fr::from(7u64);
        let node = Box::leak(Box::new(MockNode { private_logs }));
        let sender_store = Box::leak(Box::new(SenderStore::new(Arc::clone(&kv))));
        let sender_tagging_store = Box::leak(Box::new(SenderTaggingStore::new(Arc::clone(&kv))));
        let recipient_tagging_store =
            Box::leak(Box::new(RecipientTaggingStore::new(Arc::clone(&kv))));
        let capsule_store = Box::leak(Box::new(CapsuleStore::new(kv)));
        let service = LogService::new(
            node,
            sender_store,
            sender_tagging_store,
            recipient_tagging_store,
            capsule_store,
        );
        (service, recipient_tagging_store, secret)
    }

    #[tokio::test]
    async fn private_logs_preserve_requested_tag_alignment() {
        let tag_a = Fr::from(11u64);
        let tag_b = Fr::from(12u64);
        let (service, _, _) = make_service(serde_json::json!([
            [{"data": ["0x01"], "blockNumber": 5}],
            [{"data": ["0x02"], "blockNumber": 6}]
        ]));

        let logs = service
            .get_private_logs_by_tags(&[tag_a, tag_b])
            .await
            .unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0][0].tag, tag_a);
        assert_eq!(logs[1][0].tag, tag_b);
    }

    #[tokio::test]
    async fn load_logs_for_range_only_advances_index_when_logs_exist() {
        let (service, recipient_tagging_store, secret) =
            make_service(serde_json::json!([[], [], []]));
        service
            .load_logs_for_range(&AztecAddress::from(1u64), &secret, 1, 3)
            .await
            .unwrap();

        assert_eq!(
            recipient_tagging_store
                .get_highest_finalized_index(&secret)
                .await
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn bulk_retrieve_logs_preserves_request_order() {
        let (service, _, _) = make_service(serde_json::json!([
            [{"data": ["0x0a"], "blockNumber": 1}]
        ]));

        let tag_first = Fr::from(1u64);
        let tag_second = Fr::from(2u64);

        let logs = service
            .bulk_retrieve_logs(&[
                LogRetrievalRequest {
                    is_public: false,
                    tag: tag_first,
                    contract_address: None,
                },
                LogRetrievalRequest {
                    is_public: false,
                    tag: tag_second,
                    contract_address: None,
                },
            ])
            .await
            .unwrap();

        // Both requests hit private logs; ordering is verified via the tag
        // that get_private_logs_by_tags stamps on each TaggedLog.
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].len(), 1);
        assert_eq!(logs[0][0].tag, tag_first);
        assert_eq!(logs[1].len(), 1);
        assert_eq!(logs[1][0].tag, tag_second);
    }
}
