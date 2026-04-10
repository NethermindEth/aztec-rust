//! Private event store for discovered private event logs.
//!
//! Ports the TS `PrivateEventStore` with event storage, filtering by
//! contract/event selector, block range filtering, and rollback support.

use std::sync::Arc;

use aztec_core::abi::EventSelector;
use aztec_core::error::Error;
use aztec_core::tx::TxHash;
use aztec_core::types::{AztecAddress, Fr};
use serde::{Deserialize, Serialize};

use super::kv::KvStore;

/// A stored private event with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPrivateEvent {
    /// The event selector identifying the event type.
    pub event_selector: EventSelector,
    /// Randomness used in commitment.
    pub randomness: Fr,
    /// The decrypted message content fields.
    pub msg_content: Vec<Fr>,
    /// The siloed event commitment (unique identifier).
    pub siloed_event_commitment: Fr,
    /// Contract address that emitted the event.
    pub contract_address: AztecAddress,
    /// Scopes (accounts) that decrypted this event.
    pub scopes: Vec<AztecAddress>,
    /// Transaction hash.
    pub tx_hash: TxHash,
    /// L2 block number.
    pub l2_block_number: u64,
    /// L2 block hash.
    pub l2_block_hash: String,
    /// Index of the transaction within the block.
    pub tx_index_in_block: Option<u64>,
    /// Index of the event within the transaction.
    pub event_index_in_tx: Option<u64>,
}

/// Filter for querying private events.
#[derive(Debug, Clone)]
pub struct PrivateEventQueryFilter {
    /// Contract address (required).
    pub contract_address: AztecAddress,
    /// Start block (inclusive).
    pub from_block: Option<u64>,
    /// End block (inclusive).
    pub to_block: Option<u64>,
    /// Filter by scopes.
    pub scopes: Vec<AztecAddress>,
    /// Filter by transaction hash.
    pub tx_hash: Option<TxHash>,
}

/// Stores decrypted private event logs with filtering and reorg support.
pub struct PrivateEventStore {
    kv: Arc<dyn KvStore>,
}

impl PrivateEventStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    /// Store a private event log.
    pub async fn store_private_event_log(
        &self,
        event: &StoredPrivateEvent,
        scope: &AztecAddress,
    ) -> Result<(), Error> {
        let key = event_key(&event.siloed_event_commitment);

        // Check if event already exists (merge scopes)
        if let Some(existing_bytes) = self.kv.get(&key).await? {
            let mut existing: StoredPrivateEvent = serde_json::from_slice(&existing_bytes)?;
            if !existing.scopes.contains(scope) {
                existing.scopes.push(*scope);
            }
            self.kv.put(&key, &serde_json::to_vec(&existing)?).await?;
        } else {
            let mut stored = event.clone();
            if !stored.scopes.contains(scope) {
                stored.scopes.push(*scope);
            }
            self.kv.put(&key, &serde_json::to_vec(&stored)?).await?;

            // Add to contract+selector index
            self.add_to_contract_selector_index(
                &stored.contract_address,
                &stored.event_selector,
                &stored.siloed_event_commitment,
            )
            .await?;

            // Add to block number index
            self.add_to_block_index(stored.l2_block_number, &stored.siloed_event_commitment)
                .await?;
        }

        Ok(())
    }

    /// Get private events matching a filter.
    pub async fn get_private_events(
        &self,
        event_selector: &EventSelector,
        filter: &PrivateEventQueryFilter,
    ) -> Result<Vec<StoredPrivateEvent>, Error> {
        let idx_key = contract_selector_index_key(&filter.contract_address, event_selector);
        let event_ids: Vec<String> = match self.kv.get(&idx_key).await? {
            Some(bytes) => serde_json::from_slice(&bytes)?,
            None => return Ok(vec![]),
        };

        let mut events = Vec::new();
        for id_str in event_ids {
            if let Ok(id) = Fr::from_hex(&id_str) {
                let key = event_key(&id);
                if let Some(bytes) = self.kv.get(&key).await? {
                    let event: StoredPrivateEvent = serde_json::from_slice(&bytes)?;

                    // Block range filter
                    if let Some(from) = filter.from_block {
                        if event.l2_block_number < from {
                            continue;
                        }
                    }
                    if let Some(to) = filter.to_block {
                        if event.l2_block_number > to {
                            continue;
                        }
                    }

                    // Scope filter
                    if !filter.scopes.is_empty()
                        && !event.scopes.iter().any(|s| filter.scopes.contains(s))
                    {
                        continue;
                    }

                    // Tx hash filter
                    if let Some(ref tx_hash) = filter.tx_hash {
                        if event.tx_hash != *tx_hash {
                            continue;
                        }
                    }

                    events.push(event);
                }
            }
        }

        // Sort by block_number, tx_index_in_block, event_index_in_tx
        events.sort_by(|a, b| {
            a.l2_block_number
                .cmp(&b.l2_block_number)
                .then(a.tx_index_in_block.cmp(&b.tx_index_in_block))
                .then(a.event_index_in_tx.cmp(&b.event_index_in_tx))
        });

        Ok(events)
    }

    /// Rollback: delete events after a given block number.
    pub async fn rollback(
        &self,
        block_number: u64,
        _synced_block_number: u64,
    ) -> Result<(), Error> {
        let prefix = b"event_idx:block:";
        let entries = self.kv.list_prefix(prefix).await?;

        for (key, value) in &entries {
            let key_str = String::from_utf8_lossy(key);
            if let Some(bn_str) = key_str.strip_prefix("event_idx:block:") {
                if let Ok(bn) = bn_str.parse::<u64>() {
                    if bn > block_number {
                        let event_ids: Vec<String> = serde_json::from_slice(value)?;
                        for id_str in &event_ids {
                            if let Ok(id) = Fr::from_hex(id_str) {
                                let event_key = event_key(&id);
                                // Remove from contract+selector index
                                if let Some(event_bytes) = self.kv.get(&event_key).await? {
                                    let event: StoredPrivateEvent =
                                        serde_json::from_slice(&event_bytes)?;
                                    self.remove_from_contract_selector_index(
                                        &event.contract_address,
                                        &event.event_selector,
                                        &id,
                                    )
                                    .await?;
                                }
                                self.kv.delete(&event_key).await?;
                            }
                        }
                        self.kv.delete(key).await?;
                    }
                }
            }
        }

        Ok(())
    }

    // --- Index management ---

    async fn add_to_contract_selector_index(
        &self,
        contract: &AztecAddress,
        selector: &EventSelector,
        event_id: &Fr,
    ) -> Result<(), Error> {
        let key = contract_selector_index_key(contract, selector);
        let mut list: Vec<String> = match self.kv.get(&key).await? {
            Some(bytes) => serde_json::from_slice(&bytes)?,
            None => vec![],
        };
        let id_str = format!("{event_id}");
        if !list.contains(&id_str) {
            list.push(id_str);
            self.kv.put(&key, &serde_json::to_vec(&list)?).await?;
        }
        Ok(())
    }

    async fn remove_from_contract_selector_index(
        &self,
        contract: &AztecAddress,
        selector: &EventSelector,
        event_id: &Fr,
    ) -> Result<(), Error> {
        let key = contract_selector_index_key(contract, selector);
        if let Some(bytes) = self.kv.get(&key).await? {
            let mut list: Vec<String> = serde_json::from_slice(&bytes)?;
            let id_str = format!("{event_id}");
            list.retain(|s| s != &id_str);
            if list.is_empty() {
                self.kv.delete(&key).await?;
            } else {
                self.kv.put(&key, &serde_json::to_vec(&list)?).await?;
            }
        }
        Ok(())
    }

    async fn add_to_block_index(&self, block_number: u64, event_id: &Fr) -> Result<(), Error> {
        let key = block_index_key(block_number);
        let mut list: Vec<String> = match self.kv.get(&key).await? {
            Some(bytes) => serde_json::from_slice(&bytes)?,
            None => vec![],
        };
        let id_str = format!("{event_id}");
        if !list.contains(&id_str) {
            list.push(id_str);
            self.kv.put(&key, &serde_json::to_vec(&list)?).await?;
        }
        Ok(())
    }
}

fn event_key(siloed_event_commitment: &Fr) -> Vec<u8> {
    format!("event:{siloed_event_commitment}").into_bytes()
}

fn contract_selector_index_key(contract: &AztecAddress, selector: &EventSelector) -> Vec<u8> {
    format!("event_idx:cs:{contract}_{}", selector.0).into_bytes()
}

fn block_index_key(block_number: u64) -> Vec<u8> {
    format!("event_idx:block:{block_number}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::InMemoryKvStore;

    fn make_event(block: u64, commitment: u64) -> StoredPrivateEvent {
        StoredPrivateEvent {
            event_selector: EventSelector(Fr::from(0x12345678u64)),
            randomness: Fr::from(1u64),
            msg_content: vec![Fr::from(10u64)],
            siloed_event_commitment: Fr::from(commitment),
            contract_address: AztecAddress::from(1u64),
            scopes: vec![],
            tx_hash: TxHash::from_hex(
                "0x0000000000000000000000000000000000000000000000000000000000000001",
            )
            .unwrap(),
            l2_block_number: block,
            l2_block_hash: format!("0x{:064x}", block),
            tx_index_in_block: Some(0),
            event_index_in_tx: Some(0),
        }
    }

    #[tokio::test]
    async fn store_and_retrieve_events() {
        let store = PrivateEventStore::new(Arc::new(InMemoryKvStore::new()));
        let scope = AztecAddress::from(99u64);
        let event = make_event(1, 100);

        store.store_private_event_log(&event, &scope).await.unwrap();

        let selector = EventSelector(Fr::from(0x12345678u64));
        let events = store
            .get_private_events(
                &selector,
                &PrivateEventQueryFilter {
                    contract_address: AztecAddress::from(1u64),
                    from_block: None,
                    to_block: None,
                    scopes: vec![scope],
                    tx_hash: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].scopes.contains(&scope));
    }

    #[tokio::test]
    async fn block_range_filtering() {
        let store = PrivateEventStore::new(Arc::new(InMemoryKvStore::new()));
        let scope = AztecAddress::from(99u64);

        for i in 1..=5 {
            let event = make_event(i, 100 + i);
            store.store_private_event_log(&event, &scope).await.unwrap();
        }

        let selector = EventSelector(Fr::from(0x12345678u64));
        let events = store
            .get_private_events(
                &selector,
                &PrivateEventQueryFilter {
                    contract_address: AztecAddress::from(1u64),
                    from_block: Some(2),
                    to_block: Some(4),
                    scopes: vec![],
                    tx_hash: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(events.len(), 3);
    }

    #[tokio::test]
    async fn rollback_removes_events() {
        let store = PrivateEventStore::new(Arc::new(InMemoryKvStore::new()));
        let scope = AztecAddress::from(99u64);

        for i in 1..=5 {
            let event = make_event(i, 100 + i);
            store.store_private_event_log(&event, &scope).await.unwrap();
        }

        store.rollback(3, 3).await.unwrap();

        let selector = EventSelector(Fr::from(0x12345678u64));
        let events = store
            .get_private_events(
                &selector,
                &PrivateEventQueryFilter {
                    contract_address: AztecAddress::from(1u64),
                    from_block: None,
                    to_block: None,
                    scopes: vec![],
                    tx_hash: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(events.len(), 3); // blocks 1, 2, 3 remain
    }
}
