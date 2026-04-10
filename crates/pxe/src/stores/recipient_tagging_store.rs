//! Recipient tagging store for tracking incoming tag indexes.
//!
//! Ports the TS `RecipientTaggingStore` which manages aged and finalized
//! tagging indexes used in the recipient side of the tagging protocol.

use std::sync::Arc;

use aztec_core::error::Error;
use aztec_core::types::Fr;

use super::kv::KvStore;

/// Stores recipient tagging data for synchronizing recipient logs.
///
/// Tracks the highest aged index (included in block at least MAX_TX_LIFETIME
/// seconds ago, guaranteeing no new logs can appear for lower indexes) and
/// the highest finalized index (included in finalized blocks) per
/// directional app tagging secret.
pub struct RecipientTaggingStore {
    kv: Arc<dyn KvStore>,
}

impl RecipientTaggingStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    /// Get the highest aged index for a secret.
    pub async fn get_highest_aged_index(&self, secret: &Fr) -> Result<u64, Error> {
        let key = aged_key(secret);
        match self.kv.get(&key).await? {
            Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
            None => Ok(0),
        }
    }

    /// Update the highest aged index (only allows increases).
    pub async fn update_highest_aged_index(&self, secret: &Fr, index: u64) -> Result<(), Error> {
        let current = self.get_highest_aged_index(secret).await?;
        if index > current {
            let key = aged_key(secret);
            self.kv.put(&key, &serde_json::to_vec(&index)?).await?;
        }
        Ok(())
    }

    /// Get the highest finalized index for a secret.
    pub async fn get_highest_finalized_index(&self, secret: &Fr) -> Result<u64, Error> {
        let key = finalized_key(secret);
        match self.kv.get(&key).await? {
            Some(bytes) => Ok(serde_json::from_slice(&bytes)?),
            None => Ok(0),
        }
    }

    /// Update the highest finalized index (only allows increases or same).
    pub async fn update_highest_finalized_index(
        &self,
        secret: &Fr,
        index: u64,
    ) -> Result<(), Error> {
        let current = self.get_highest_finalized_index(secret).await?;
        if index < current {
            return Err(Error::InvalidData(format!(
                "cannot lower finalized index from {} to {}",
                current, index
            )));
        }
        if index > current {
            let key = finalized_key(secret);
            self.kv.put(&key, &serde_json::to_vec(&index)?).await?;
        }
        Ok(())
    }
}

fn aged_key(secret: &Fr) -> Vec<u8> {
    format!("recipient_tag:aged:{secret}").into_bytes()
}

fn finalized_key(secret: &Fr) -> Vec<u8> {
    format!("recipient_tag:finalized:{secret}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::InMemoryKvStore;

    fn make_store() -> RecipientTaggingStore {
        RecipientTaggingStore::new(Arc::new(InMemoryKvStore::new()))
    }

    #[tokio::test]
    async fn default_indexes_are_zero() {
        let store = make_store();
        let secret = Fr::from(42u64);
        assert_eq!(store.get_highest_aged_index(&secret).await.unwrap(), 0);
        assert_eq!(store.get_highest_finalized_index(&secret).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn update_aged_index() {
        let store = make_store();
        let secret = Fr::from(42u64);

        store.update_highest_aged_index(&secret, 5).await.unwrap();
        assert_eq!(store.get_highest_aged_index(&secret).await.unwrap(), 5);

        // Lower value is ignored
        store.update_highest_aged_index(&secret, 3).await.unwrap();
        assert_eq!(store.get_highest_aged_index(&secret).await.unwrap(), 5);

        // Higher value updates
        store.update_highest_aged_index(&secret, 10).await.unwrap();
        assert_eq!(store.get_highest_aged_index(&secret).await.unwrap(), 10);
    }

    #[tokio::test]
    async fn update_finalized_index() {
        let store = make_store();
        let secret = Fr::from(42u64);

        store
            .update_highest_finalized_index(&secret, 5)
            .await
            .unwrap();
        assert_eq!(store.get_highest_finalized_index(&secret).await.unwrap(), 5);

        // Same value is accepted
        store
            .update_highest_finalized_index(&secret, 5)
            .await
            .unwrap();

        // Lower value is rejected
        let result = store.update_highest_finalized_index(&secret, 3).await;
        assert!(result.is_err());
    }
}
