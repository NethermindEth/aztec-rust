//! Block state synchronizer with reorg handling.
//!
//! Ports the TS `BlockSynchronizer` which handles block header updates,
//! chain reorganization detection, and coordinated rollback of NoteStore
//! and PrivateEventStore.

use std::sync::Arc;

use aztec_core::error::Error;
use aztec_node_client::AztecNode;
use tokio::sync::RwLock;

use crate::stores::anchor_block_store::AnchorBlockHeader;
use crate::stores::{AnchorBlockStore, NoteStore, PrivateEventStore};

/// Which chain tip the block synchronizer should track.
///
/// Only `Proposed` and `Proven` are supported. Upstream does not expose
/// separate "checkpointed" or "finalized" block numbers through the node API,
/// so those modes are intentionally excluded to avoid misleading configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncChainTip {
    /// Track the latest proposed (tip) block.
    Proposed,
    /// Track the latest proven block.
    Proven,
}

/// Configuration for the block state synchronizer.
#[derive(Debug, Clone)]
pub struct BlockSyncConfig {
    /// Which chain tip to sync to.
    pub sync_chain_tip: SyncChainTip,
}

impl Default for BlockSyncConfig {
    fn default() -> Self {
        Self {
            sync_chain_tip: SyncChainTip::Proposed,
        }
    }
}

/// Block state synchronizer that manages block header tracking and reorg handling.
///
/// Matches the TS `BlockSynchronizer`:
/// - Syncs the anchor block header from the node
/// - Detects chain reorganizations (when the node's block number < our anchor)
/// - Rolls back NoteStore and PrivateEventStore on reorg
/// - Signals ContractSyncService cache invalidation
///
/// The synchronizer ensures that all PXE state is consistent with the
/// node's view of the chain.
pub struct BlockStateSynchronizer {
    anchor_block_store: Arc<AnchorBlockStore>,
    note_store: Arc<NoteStore>,
    private_event_store: Arc<PrivateEventStore>,
    config: BlockSyncConfig,
    /// Flag indicating a sync is in progress (prevents concurrent syncs).
    syncing: RwLock<bool>,
    /// Callback-style flag: set to true when the anchor block changes,
    /// so that callers (e.g., EmbeddedPxe) can wipe the ContractSyncService.
    anchor_changed: RwLock<bool>,
}

impl BlockStateSynchronizer {
    pub fn new(
        anchor_block_store: Arc<AnchorBlockStore>,
        note_store: Arc<NoteStore>,
        private_event_store: Arc<PrivateEventStore>,
        config: BlockSyncConfig,
    ) -> Self {
        Self {
            anchor_block_store,
            note_store,
            private_event_store,
            config,
            syncing: RwLock::new(false),
            anchor_changed: RwLock::new(false),
        }
    }

    /// Sync the PXE with the node's current state.
    ///
    /// This is the main entry point called before transaction simulation
    /// or event retrieval. It:
    /// 1. Fetches the latest block header from the node
    /// 2. Detects if a reorg has occurred
    /// 3. Handles rollback if needed
    /// 4. Updates the anchor block header
    pub async fn sync<N: AztecNode>(&self, node: &N) -> Result<(), Error> {
        // Prevent concurrent syncs
        {
            let mut syncing = self.syncing.write().await;
            if *syncing {
                // Wait for the current sync to finish by polling
                drop(syncing);
                loop {
                    let s = self.syncing.read().await;
                    if !*s {
                        break;
                    }
                    drop(s);
                    tokio::task::yield_now().await;
                }
                return Ok(());
            }
            *syncing = true;
        }

        let result = self.do_sync(node).await;

        *self.syncing.write().await = false;

        result
    }

    /// Internal sync implementation.
    async fn do_sync<N: AztecNode>(&self, node: &N) -> Result<(), Error> {
        // Ensure we have an initial anchor block header
        let current_anchor = self.anchor_block_store.get_block_header().await?;
        if current_anchor.is_none() {
            // First sync: fetch block 0 (genesis) header as initial anchor
            let genesis_header = node.get_block_header(0).await?;
            let anchor = AnchorBlockHeader::from_header_json(genesis_header);
            self.update_anchor_block_header(&anchor).await?;
        }

        // Fetch the latest block number from the node based on sync config
        let latest_block_number = match self.config.sync_chain_tip {
            SyncChainTip::Proven => node.get_proven_block_number().await?,
            SyncChainTip::Proposed => node.get_block_number().await?,
        };

        let current_anchor = self.anchor_block_store.get_block_header().await?;
        let current_anchor_block_number =
            current_anchor.as_ref().map(|a| a.block_number).unwrap_or(0);

        // Check for reorg: if the node's latest block is behind our anchor
        if latest_block_number < current_anchor_block_number && current_anchor_block_number > 0 {
            tracing::warn!(
                current_anchor = current_anchor_block_number,
                node_latest = latest_block_number,
                "detected chain reorg (block number behind) — rolling back"
            );
            self.handle_reorg(node, latest_block_number, current_anchor_block_number)
                .await?;
        } else if latest_block_number >= current_anchor_block_number
            && current_anchor_block_number > 0
        {
            // Same or higher block number — check for same-height reorg by
            // comparing the block hash at our current anchor height.
            // Upstream uses the block stream's chain-pruned event keyed by
            // both number and hash; we approximate by fetching the header at
            // the anchor height and comparing hashes.
            let stored_hash = current_anchor
                .as_ref()
                .map(|a| a.block_hash.as_str())
                .unwrap_or("");

            if !stored_hash.is_empty() && stored_hash != "0x0" {
                let remote_header = node.get_block_header(current_anchor_block_number).await?;
                let remote_hash = remote_header
                    .pointer("/blockHash")
                    .or_else(|| remote_header.get("blockHash"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if !remote_hash.is_empty() && remote_hash != stored_hash {
                    tracing::warn!(
                        current_anchor = current_anchor_block_number,
                        stored_hash = stored_hash,
                        remote_hash = remote_hash,
                        "detected same-height chain reorg — rolling back"
                    );
                    // Roll back to the block before our anchor, then advance
                    let rollback_to = current_anchor_block_number.saturating_sub(1);
                    self.handle_reorg(node, rollback_to, current_anchor_block_number)
                        .await?;
                    // Fall through to advance below
                }
            }

            // Re-read anchor after potential reorg handling
            let anchor_after = self.anchor_block_store.get_block_number().await?;
            if latest_block_number > anchor_after {
                let new_header = node.get_block_header(latest_block_number).await?;
                let anchor = AnchorBlockHeader::from_header_json(new_header);
                self.update_anchor_block_header(&anchor).await?;
            }
        }

        Ok(())
    }

    /// Handle a chain reorganization.
    ///
    /// Matching TS `chain-pruned` event handler:
    /// 1. Roll back NoteStore (un-nullify orphaned notes, delete new notes)
    /// 2. Roll back PrivateEventStore (delete events from orphaned blocks)
    /// 3. Update anchor block header to the new tip
    async fn handle_reorg<N: AztecNode>(
        &self,
        node: &N,
        new_block_number: u64,
        old_block_number: u64,
    ) -> Result<(), Error> {
        tracing::warn!(
            "pruning data after block {new_block_number} due to reorg \
             (was synced to block {old_block_number})"
        );

        // Fetch the new anchor block header
        let new_header = node.get_block_header(new_block_number).await?;
        let anchor = AnchorBlockHeader::from_header_json(new_header);

        // Roll back stores atomically (best-effort atomicity via sequential ops)
        self.note_store
            .rollback(new_block_number, old_block_number)
            .await?;

        self.private_event_store
            .rollback(new_block_number, old_block_number)
            .await?;

        // Update anchor
        self.update_anchor_block_header(&anchor).await?;

        tracing::info!(
            "reorg handled: rolled back from block {old_block_number} to {new_block_number}"
        );

        Ok(())
    }

    /// Update the anchor block header and signal that it changed.
    async fn update_anchor_block_header(&self, header: &AnchorBlockHeader) -> Result<(), Error> {
        self.anchor_block_store.set_header(header).await?;
        *self.anchor_changed.write().await = true;
        tracing::debug!(
            block_number = header.block_number,
            "updated anchor block header"
        );
        Ok(())
    }

    /// Check and consume the anchor-changed flag.
    ///
    /// Returns `true` if the anchor block has changed since the last call.
    /// The flag is reset to `false` after reading.
    /// This is used by EmbeddedPxe to know when to wipe the ContractSyncService cache.
    pub async fn take_anchor_changed(&self) -> bool {
        let mut changed = self.anchor_changed.write().await;
        let was_changed = *changed;
        *changed = false;
        was_changed
    }

    /// Get a reference to the anchor block store.
    pub fn anchor_block_store(&self) -> &AnchorBlockStore {
        &self.anchor_block_store
    }

    /// Get the current anchor block header.
    pub async fn get_anchor_block_header(&self) -> Result<Option<AnchorBlockHeader>, Error> {
        self.anchor_block_store.get_block_header().await
    }

    /// Get the current anchor block number.
    pub async fn get_anchor_block_number(&self) -> Result<u64, Error> {
        self.anchor_block_store.get_block_number().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::InMemoryKvStore;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct MockNode {
        block_number: AtomicU64,
    }

    impl MockNode {
        fn new(block: u64) -> Self {
            Self {
                block_number: AtomicU64::new(block),
            }
        }

        fn set_block_number(&self, n: u64) {
            self.block_number.store(n, Ordering::SeqCst);
        }
    }

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
            Ok(self.block_number.load(Ordering::SeqCst))
        }
        async fn get_proven_block_number(&self) -> Result<u64, Error> {
            Ok(self.block_number.load(Ordering::SeqCst))
        }
        async fn get_tx_receipt(
            &self,
            _: &aztec_core::tx::TxHash,
        ) -> Result<aztec_core::tx::TxReceipt, Error> {
            Err(Error::InvalidData("mock".into()))
        }
        async fn get_tx_effect(
            &self,
            _: &aztec_core::tx::TxHash,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_tx_by_hash(
            &self,
            _: &aztec_core::tx::TxHash,
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
            Err(Error::InvalidData("mock".into()))
        }
        async fn get_contract(
            &self,
            _: &aztec_core::types::AztecAddress,
        ) -> Result<Option<aztec_core::types::ContractInstanceWithAddress>, Error> {
            Ok(None)
        }
        async fn get_contract_class(
            &self,
            _: &aztec_core::types::Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_block_header(&self, block_number: u64) -> Result<serde_json::Value, Error> {
            let bn = if block_number == 0 {
                self.block_number.load(Ordering::SeqCst)
            } else {
                block_number
            };
            Ok(serde_json::json!({
                "globalVariables": {"blockNumber": bn},
                "blockHash": format!("0x{:064x}", bn)
            }))
        }
        async fn get_block(&self, _: u64) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_note_hash_membership_witness(
            &self,
            _: u64,
            _: &aztec_core::types::Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_nullifier_membership_witness(
            &self,
            _: u64,
            _: &aztec_core::types::Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_low_nullifier_membership_witness(
            &self,
            _: u64,
            _: &aztec_core::types::Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_public_storage_at(
            &self,
            _: u64,
            _: &aztec_core::types::AztecAddress,
            _: &aztec_core::types::Fr,
        ) -> Result<aztec_core::types::Fr, Error> {
            Ok(aztec_core::types::Fr::zero())
        }
        async fn get_public_data_witness(
            &self,
            _: u64,
            _: &aztec_core::types::Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_l1_to_l2_message_membership_witness(
            &self,
            _: u64,
            _: &aztec_core::types::Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
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
        async fn get_private_logs_by_tags(
            &self,
            _: &[aztec_core::types::Fr],
        ) -> Result<serde_json::Value, Error> {
            Ok(serde_json::json!([]))
        }
        async fn get_public_logs_by_tags_from_contract(
            &self,
            _: &aztec_core::types::AztecAddress,
            _: &[aztec_core::types::Fr],
        ) -> Result<serde_json::Value, Error> {
            Ok(serde_json::json!([]))
        }
        async fn register_contract_function_signatures(&self, _: &[String]) -> Result<(), Error> {
            Ok(())
        }
        async fn get_block_hash_membership_witness(
            &self,
            _: u64,
            _: &aztec_core::types::Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn find_leaves_indexes(
            &self,
            _: u64,
            _: &str,
            _: &[aztec_core::types::Fr],
        ) -> Result<Vec<Option<u64>>, Error> {
            Ok(vec![])
        }
    }

    fn make_synchronizer() -> (BlockStateSynchronizer, Arc<AnchorBlockStore>) {
        let kv: Arc<dyn crate::stores::kv::KvStore> = Arc::new(InMemoryKvStore::new());
        let anchor_store = Arc::new(AnchorBlockStore::new(Arc::clone(&kv)));
        let note_store = Arc::new(NoteStore::new(Arc::clone(&kv)));
        let event_store = Arc::new(PrivateEventStore::new(Arc::clone(&kv)));
        let sync = BlockStateSynchronizer::new(
            Arc::clone(&anchor_store),
            note_store,
            event_store,
            BlockSyncConfig::default(),
        );
        (sync, anchor_store)
    }

    #[tokio::test]
    async fn first_sync_sets_anchor() {
        let (sync, anchor_store) = make_synchronizer();
        let node = MockNode::new(5);

        sync.sync(&node).await.unwrap();

        let anchor = anchor_store.get_block_header().await.unwrap().unwrap();
        assert_eq!(anchor.block_number, 5);
    }

    #[tokio::test]
    async fn sync_advances_anchor() {
        let (sync, anchor_store) = make_synchronizer();
        let node = MockNode::new(5);

        sync.sync(&node).await.unwrap();
        assert_eq!(
            anchor_store
                .get_block_header()
                .await
                .unwrap()
                .unwrap()
                .block_number,
            5
        );

        node.set_block_number(10);
        sync.sync(&node).await.unwrap();
        assert_eq!(
            anchor_store
                .get_block_header()
                .await
                .unwrap()
                .unwrap()
                .block_number,
            10
        );
    }

    #[tokio::test]
    async fn sync_detects_reorg_and_rolls_back() {
        let (sync, anchor_store) = make_synchronizer();
        let node = MockNode::new(10);

        sync.sync(&node).await.unwrap();
        assert_eq!(
            anchor_store
                .get_block_header()
                .await
                .unwrap()
                .unwrap()
                .block_number,
            10
        );

        // Simulate reorg: node goes back to block 7
        node.set_block_number(7);
        sync.sync(&node).await.unwrap();
        assert_eq!(
            anchor_store
                .get_block_header()
                .await
                .unwrap()
                .unwrap()
                .block_number,
            7
        );
    }

    #[tokio::test]
    async fn take_anchor_changed_flag() {
        let (sync, _) = make_synchronizer();
        let node = MockNode::new(5);

        sync.sync(&node).await.unwrap();
        assert!(sync.take_anchor_changed().await);
        assert!(!sync.take_anchor_changed().await); // consumed
    }

    #[tokio::test]
    async fn no_update_when_same_block() {
        let (sync, _) = make_synchronizer();
        let node = MockNode::new(5);

        sync.sync(&node).await.unwrap();
        assert!(sync.take_anchor_changed().await);

        // Same block number: no change
        sync.sync(&node).await.unwrap();
        assert!(!sync.take_anchor_changed().await);
    }
}
