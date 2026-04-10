//! Sender address storage for private log discovery.

use std::sync::Arc;

use aztec_core::error::Error;
use aztec_core::types::{AztecAddress, Fr};

use super::kv::KvStore;

/// Stores registered sender addresses for private log discovery.
pub struct SenderStore {
    kv: Arc<dyn KvStore>,
}

impl SenderStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    /// Register a sender address.
    pub async fn add(&self, sender: &AztecAddress) -> Result<(), Error> {
        let key = sender_key(sender);
        // Store a minimal value — presence of the key is what matters.
        self.kv.put(&key, &[1]).await
    }

    /// Remove a sender address.
    pub async fn remove(&self, sender: &AztecAddress) -> Result<(), Error> {
        let key = sender_key(sender);
        self.kv.delete(&key).await
    }

    /// Check if a sender is registered.
    pub async fn contains(&self, sender: &AztecAddress) -> Result<bool, Error> {
        let key = sender_key(sender);
        Ok(self.kv.get(&key).await?.is_some())
    }

    /// List all registered sender addresses.
    pub async fn get_all(&self) -> Result<Vec<AztecAddress>, Error> {
        let entries = self.kv.list_prefix(b"sender:").await?;
        entries
            .into_iter()
            .map(|(k, _)| {
                let key_str = String::from_utf8_lossy(&k);
                let hex_part = key_str
                    .strip_prefix("sender:")
                    .ok_or_else(|| Error::InvalidData("invalid sender key prefix".into()))?;
                Ok(AztecAddress(Fr::from_hex(hex_part)?))
            })
            .collect()
    }
}

fn sender_key(sender: &AztecAddress) -> Vec<u8> {
    format!("sender:{sender}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::InMemoryKvStore;

    #[tokio::test]
    async fn add_remove_senders() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = SenderStore::new(kv);
        let sender = AztecAddress::from(99u64);

        store.add(&sender).await.unwrap();
        assert!(store.contains(&sender).await.unwrap());

        let all = store.get_all().await.unwrap();
        assert_eq!(all.len(), 1);

        store.remove(&sender).await.unwrap();
        assert!(!store.contains(&sender).await.unwrap());
        assert!(store.get_all().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn add_is_idempotent() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = SenderStore::new(kv);
        let sender = AztecAddress::from(42u64);

        store.add(&sender).await.unwrap();
        store.add(&sender).await.unwrap();

        let all = store.get_all().await.unwrap();
        assert_eq!(all.len(), 1);
    }
}
