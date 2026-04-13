//! Contract sync service for note discovery.
//!
//! Ports the TS `ContractSyncService` which ensures a contract's private state
//! is synchronized at the PXE level. Runs `sync_state` utility function and
//! syncs note nullifiers.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;

use aztec_core::error::Error;
use aztec_core::types::AztecAddress;
use aztec_node_client::AztecNode;
use tokio::sync::RwLock;

use crate::stores::NoteStore;
use crate::sync::note_service::NoteService;

/// Service that ensures contracts' private state is synced before execution.
///
/// Maintains a cache of already-synced contracts to avoid redundant sync
/// operations within a transaction.
pub struct ContractSyncService<N: AztecNode> {
    node: Arc<N>,
    note_store: Arc<NoteStore>,
    /// Set of (contract, scope) pairs that have been synced.
    synced: RwLock<HashSet<String>>,
    /// Current anchor block hash — cache is cleared when this changes.
    anchor_block_hash: RwLock<Option<String>>,
}

impl<N: AztecNode> ContractSyncService<N> {
    pub fn new(node: Arc<N>, note_store: Arc<NoteStore>) -> Self {
        Self {
            node,
            note_store,
            synced: RwLock::new(HashSet::new()),
            anchor_block_hash: RwLock::new(None),
        }
    }

    /// Ensure a contract's private state is synchronized.
    ///
    /// If the contract has already been synced for the given scopes in the
    /// current anchor block, this is a no-op.
    pub async fn ensure_contract_synced<F, Fut>(
        &self,
        contract_address: &AztecAddress,
        scopes: &[AztecAddress],
        anchor_block_hash: &str,
        utility_executor: F,
    ) -> Result<(), Error>
    where
        F: Fn(AztecAddress, Vec<AztecAddress>) -> Fut,
        Fut: Future<Output = Result<(), Error>>,
    {
        self.ensure_contract_synced_with(
            contract_address,
            scopes,
            anchor_block_hash,
            &utility_executor,
        )
        .await
    }

    pub async fn ensure_contract_synced_with<F, Fut>(
        &self,
        contract_address: &AztecAddress,
        scopes: &[AztecAddress],
        anchor_block_hash: &str,
        utility_executor: &F,
    ) -> Result<(), Error>
    where
        F: Fn(AztecAddress, Vec<AztecAddress>) -> Fut,
        Fut: Future<Output = Result<(), Error>>,
    {
        // Check if anchor block changed — clear cache
        {
            let mut cached_hash = self.anchor_block_hash.write().await;
            if cached_hash.as_deref() != Some(anchor_block_hash) {
                *cached_hash = Some(anchor_block_hash.to_owned());
                self.synced.write().await.clear();
            }
        }

        // Check if already synced for these scopes
        let unsynced_scopes = {
            let synced = self.synced.read().await;
            scopes
                .iter()
                .filter(|scope| {
                    let key = sync_key(contract_address, scope);
                    let wildcard = sync_key_wildcard(contract_address);
                    !synced.contains(&key) && !synced.contains(&wildcard)
                })
                .cloned()
                .collect::<Vec<_>>()
        };

        if unsynced_scopes.is_empty() {
            return Ok(());
        }

        // Do the sync
        self.do_sync(contract_address, &unsynced_scopes, utility_executor)
            .await?;

        // Mark as synced
        {
            let mut synced = self.synced.write().await;
            for scope in &unsynced_scopes {
                synced.insert(sync_key(contract_address, scope));
            }
        }

        Ok(())
    }

    /// Perform the actual sync: run sync_state and sync nullifiers in parallel.
    async fn do_sync<F, Fut>(
        &self,
        contract_address: &AztecAddress,
        scopes: &[AztecAddress],
        utility_executor: &F,
    ) -> Result<(), Error>
    where
        F: Fn(AztecAddress, Vec<AztecAddress>) -> Fut,
        Fut: Future<Output = Result<(), Error>>,
    {
        tracing::debug!(
            contract = %contract_address,
            scopes = scopes.len(),
            "syncing contract state"
        );

        let note_service = NoteService::new(&*self.node, &self.note_store);
        // Use the latest block from the node for nullifier lookups
        let anchor_block = self.node.get_block_number().await.unwrap_or(0);

        let nullified_future =
            note_service.sync_note_nullifiers(contract_address, scopes, anchor_block);
        let sync_state_future = utility_executor(*contract_address, scopes.to_vec());
        let (nullified, ()) = tokio::try_join!(nullified_future, sync_state_future)?;

        if nullified > 0 {
            tracing::debug!(
                contract = %contract_address,
                nullified = nullified,
                "nullified stale notes"
            );
        }

        Ok(())
    }

    /// Clear the sync cache (e.g., on reorg or anchor block change).
    pub async fn wipe(&self) {
        self.synced.write().await.clear();
        *self.anchor_block_hash.write().await = None;
    }
}

fn sync_key(contract: &AztecAddress, scope: &AztecAddress) -> String {
    format!("{contract}:{scope}")
}

fn sync_key_wildcard(contract: &AztecAddress) -> String {
    format!("{contract}:*")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::InMemoryKvStore;

    // A minimal mock for testing sync service
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
            Ok(1)
        }
        async fn get_proven_block_number(&self) -> Result<u64, Error> {
            Ok(1)
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
            _: &AztecAddress,
        ) -> Result<Option<aztec_core::types::ContractInstanceWithAddress>, Error> {
            Ok(None)
        }
        async fn get_contract_class(
            &self,
            _: &aztec_core::types::Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
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
            _: &AztecAddress,
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
            _: &AztecAddress,
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

    #[tokio::test]
    async fn sync_is_idempotent() {
        let node = Arc::new(MockNode);
        let kv = Arc::new(InMemoryKvStore::new());
        let note_store = Arc::new(NoteStore::new(kv));
        let service = ContractSyncService::new(node, note_store);

        let contract = AztecAddress::from(1u64);
        let scope = AztecAddress::from(99u64);

        // First sync should succeed
        service
            .ensure_contract_synced(
                &contract,
                &[scope],
                "block_hash_1",
                |_contract, _scopes| async { Ok(()) },
            )
            .await
            .unwrap();

        // Second sync with same params is a no-op (cached)
        service
            .ensure_contract_synced(
                &contract,
                &[scope],
                "block_hash_1",
                |_contract, _scopes| async { Ok(()) },
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn sync_cache_clears_on_new_block() {
        let node = Arc::new(MockNode);
        let kv = Arc::new(InMemoryKvStore::new());
        let note_store = Arc::new(NoteStore::new(kv));
        let service = ContractSyncService::new(node, note_store);

        let contract = AztecAddress::from(1u64);
        let scope = AztecAddress::from(99u64);

        service
            .ensure_contract_synced(
                &contract,
                &[scope],
                "block_hash_1",
                |_contract, _scopes| async { Ok(()) },
            )
            .await
            .unwrap();

        // New block hash clears cache, sync runs again
        service
            .ensure_contract_synced(
                &contract,
                &[scope],
                "block_hash_2",
                |_contract, _scopes| async { Ok(()) },
            )
            .await
            .unwrap();
    }
}
