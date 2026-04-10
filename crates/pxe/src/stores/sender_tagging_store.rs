//! Sender tagging store for tracking outgoing tag indexes.
//!
//! Ports the TS `SenderTaggingStore` which manages pending and finalized
//! tagging indexes used in the sender side of the tagging protocol.

use std::collections::HashMap;
use std::sync::Arc;

use aztec_core::error::Error;
use aztec_core::tx::TxHash;
use aztec_core::types::Fr;
use serde::{Deserialize, Serialize};

use super::kv::KvStore;

/// Maximum distance between a pending index and the last finalized index.
const UNFINALIZED_TAGGING_INDEXES_WINDOW_LEN: u64 = 20;

/// A pending tag index associated with a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingTagIndex {
    /// The tagging index value.
    pub index: u64,
    /// The transaction hash that used this tag.
    pub tx_hash: TxHash,
}

/// Pre-tag entry stored before a transaction is sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreTag {
    /// The directional app tagging secret (as field element).
    pub secret: Fr,
    /// The tagging index used.
    pub index: u64,
    /// The transaction hash.
    pub tx_hash: TxHash,
}

/// Stores sender tagging data for synchronizing sender tagging indexes.
///
/// Tracks pending indexes (used but not yet finalized) and finalized indexes
/// (included in proven blocks) per directional app tagging secret.
pub struct SenderTaggingStore {
    kv: Arc<dyn KvStore>,
}

impl SenderTaggingStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    /// Store pending indexes from pre-tags.
    ///
    /// Validates:
    /// - No duplicate secrets in pre-tags
    /// - Index is within WINDOW_LEN of finalized index
    /// - Index is greater than finalized index
    /// - No duplicate secret+txHash pairs with different indexes
    pub async fn store_pending_indexes(&self, pre_tags: &[PreTag]) -> Result<(), Error> {
        // Check for duplicate secrets
        let mut seen_secrets = std::collections::HashSet::new();
        for tag in pre_tags {
            if !seen_secrets.insert(tag.secret) {
                return Err(Error::InvalidData(format!(
                    "duplicate secret in pre-tags: {}",
                    tag.secret
                )));
            }
        }

        for tag in pre_tags {
            let finalized = self.get_last_finalized_index(&tag.secret).await?;

            // Validate index is not too far ahead
            if tag.index > finalized + UNFINALIZED_TAGGING_INDEXES_WINDOW_LEN {
                return Err(Error::InvalidData(format!(
                    "pending index {} is too far ahead of finalized index {} (window={})",
                    tag.index, finalized, UNFINALIZED_TAGGING_INDEXES_WINDOW_LEN
                )));
            }

            // Validate index is greater than finalized
            if tag.index <= finalized {
                return Err(Error::InvalidData(format!(
                    "pending index {} is not greater than finalized index {}",
                    tag.index, finalized
                )));
            }

            // Check for conflicting existing entries
            let existing = self.get_pending_indexes(&tag.secret).await?;
            for entry in &existing {
                if entry.tx_hash == tag.tx_hash && entry.index != tag.index {
                    return Err(Error::InvalidData(format!(
                        "conflicting pending index: secret={} tx_hash={} existing_index={} new_index={}",
                        tag.secret, tag.tx_hash, entry.index, tag.index
                    )));
                }
            }

            // Store the pending index (keep highest per secret+tx pair)
            let entry = PendingTagIndex {
                index: tag.index,
                tx_hash: tag.tx_hash,
            };
            let mut pending = existing;
            // Remove any existing entry for this tx_hash (keep highest)
            pending.retain(|e| e.tx_hash != tag.tx_hash);
            pending.push(entry);

            let key = pending_key(&tag.secret);
            let value = serde_json::to_vec(&pending)?;
            self.kv.put(&key, &value).await?;
        }

        Ok(())
    }

    /// Get transaction hashes of pending indexes within a range.
    pub async fn get_tx_hashes_of_pending_indexes(
        &self,
        secret: &Fr,
        from_index: u64,
        to_index: u64,
    ) -> Result<Vec<TxHash>, Error> {
        let pending = self.get_pending_indexes(secret).await?;
        Ok(pending
            .into_iter()
            .filter(|e| e.index >= from_index && e.index <= to_index)
            .map(|e| e.tx_hash)
            .collect())
    }

    /// Get the last finalized index for a secret.
    pub async fn get_last_finalized_index(&self, secret: &Fr) -> Result<u64, Error> {
        let key = finalized_key(secret);
        match self.kv.get(&key).await? {
            Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
            None => Ok(0),
        }
    }

    /// Get the last used index (max of pending and finalized).
    pub async fn get_last_used_index(&self, secret: &Fr) -> Result<u64, Error> {
        let finalized = self.get_last_finalized_index(secret).await?;
        let pending = self.get_pending_indexes(secret).await?;
        let max_pending = pending.iter().map(|e| e.index).max().unwrap_or(0);
        Ok(finalized.max(max_pending))
    }

    /// Drop pending indexes for dropped transactions.
    pub async fn drop_pending_indexes(&self, tx_hashes: &[TxHash]) -> Result<(), Error> {
        let prefix = b"sender_tag:pending:";
        let entries = self.kv.list_prefix(prefix).await?;

        for (key, value) in entries {
            let mut pending: Vec<PendingTagIndex> = serde_json::from_slice(&value)?;
            let original_len = pending.len();
            pending.retain(|e| !tx_hashes.contains(&e.tx_hash));
            if pending.len() != original_len {
                if pending.is_empty() {
                    self.kv.delete(&key).await?;
                } else {
                    self.kv.put(&key, &serde_json::to_vec(&pending)?).await?;
                }
            }
        }

        Ok(())
    }

    /// Finalize pending indexes: mark them as finalized and prune lower ones.
    pub async fn finalize_pending_indexes(
        &self,
        secret: &Fr,
        up_to_index: u64,
    ) -> Result<(), Error> {
        // Update finalized index
        let current = self.get_last_finalized_index(secret).await?;
        if up_to_index > current {
            let key = finalized_key(secret);
            self.kv
                .put(&key, &serde_json::to_vec(&up_to_index)?)
                .await?;
        }

        // Remove finalized entries from pending
        let mut pending = self.get_pending_indexes(secret).await?;
        pending.retain(|e| e.index > up_to_index);
        let key = pending_key(secret);
        if pending.is_empty() {
            self.kv.delete(&key).await?;
        } else {
            self.kv.put(&key, &serde_json::to_vec(&pending)?).await?;
        }

        Ok(())
    }

    /// Get all pending indexes for a secret.
    async fn get_pending_indexes(&self, secret: &Fr) -> Result<Vec<PendingTagIndex>, Error> {
        let key = pending_key(secret);
        match self.kv.get(&key).await? {
            Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
            None => Ok(vec![]),
        }
    }

    /// Get pending indexes as a map (secret → entries).
    pub async fn get_all_pending(&self) -> Result<HashMap<Fr, Vec<PendingTagIndex>>, Error> {
        let prefix = b"sender_tag:pending:";
        let entries = self.kv.list_prefix(prefix).await?;
        let mut result = HashMap::new();
        for (key, value) in entries {
            let key_str = String::from_utf8_lossy(&key);
            if let Some(secret_str) = key_str.strip_prefix("sender_tag:pending:") {
                if let Ok(secret) = Fr::from_hex(secret_str) {
                    let pending: Vec<PendingTagIndex> = serde_json::from_slice(&value)?;
                    result.insert(secret, pending);
                }
            }
        }
        Ok(result)
    }
}

fn pending_key(secret: &Fr) -> Vec<u8> {
    format!("sender_tag:pending:{secret}").into_bytes()
}

fn finalized_key(secret: &Fr) -> Vec<u8> {
    format!("sender_tag:finalized:{secret}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::InMemoryKvStore;

    fn make_store() -> SenderTaggingStore {
        SenderTaggingStore::new(Arc::new(InMemoryKvStore::new()))
    }

    #[tokio::test]
    async fn store_and_retrieve_pending_indexes() {
        let store = make_store();
        let secret = Fr::from(42u64);
        let tx_hash =
            TxHash::from_hex("0x0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap();

        store
            .store_pending_indexes(&[PreTag {
                secret,
                index: 1,
                tx_hash,
            }])
            .await
            .unwrap();

        let last = store.get_last_used_index(&secret).await.unwrap();
        assert_eq!(last, 1);

        let hashes = store
            .get_tx_hashes_of_pending_indexes(&secret, 0, 5)
            .await
            .unwrap();
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0], tx_hash);
    }

    #[tokio::test]
    async fn finalize_removes_from_pending() {
        let store = make_store();
        let secret = Fr::from(42u64);
        let tx_hash =
            TxHash::from_hex("0x0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap();

        store
            .store_pending_indexes(&[PreTag {
                secret,
                index: 1,
                tx_hash,
            }])
            .await
            .unwrap();

        store.finalize_pending_indexes(&secret, 1).await.unwrap();

        let finalized = store.get_last_finalized_index(&secret).await.unwrap();
        assert_eq!(finalized, 1);

        let hashes = store
            .get_tx_hashes_of_pending_indexes(&secret, 0, 5)
            .await
            .unwrap();
        assert!(hashes.is_empty());
    }

    #[tokio::test]
    async fn drop_pending_by_tx_hash() {
        let store = make_store();
        let secret = Fr::from(42u64);
        let tx1 =
            TxHash::from_hex("0x0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap();
        let tx2 =
            TxHash::from_hex("0x0000000000000000000000000000000000000000000000000000000000000002")
                .unwrap();

        store
            .store_pending_indexes(&[PreTag {
                secret,
                index: 1,
                tx_hash: tx1,
            }])
            .await
            .unwrap();
        // Store second with different secret to avoid duplicate validation
        let secret2 = Fr::from(43u64);
        store
            .store_pending_indexes(&[PreTag {
                secret: secret2,
                index: 1,
                tx_hash: tx2,
            }])
            .await
            .unwrap();

        store.drop_pending_indexes(&[tx1]).await.unwrap();

        let hashes = store
            .get_tx_hashes_of_pending_indexes(&secret, 0, 5)
            .await
            .unwrap();
        assert!(hashes.is_empty());

        let hashes2 = store
            .get_tx_hashes_of_pending_indexes(&secret2, 0, 5)
            .await
            .unwrap();
        assert_eq!(hashes2.len(), 1);
    }

    #[tokio::test]
    async fn rejects_index_beyond_window() {
        let store = make_store();
        let secret = Fr::from(42u64);
        let tx_hash =
            TxHash::from_hex("0x0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap();

        let result = store
            .store_pending_indexes(&[PreTag {
                secret,
                index: UNFINALIZED_TAGGING_INDEXES_WINDOW_LEN + 1,
                tx_hash,
            }])
            .await;
        assert!(result.is_err());
    }
}
