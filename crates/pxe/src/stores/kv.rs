//! Key-value store abstraction for PXE local state.

use std::collections::BTreeMap;
use std::sync::RwLock;

use async_trait::async_trait;
use aztec_core::error::Error;

/// Simple key-value store trait (async for future flexibility).
#[async_trait]
pub trait KvStore: Send + Sync {
    /// Get a value by key.
    async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Error>;
    /// Put a key-value pair.
    async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), Error>;
    /// Delete a key.
    async fn delete(&self, key: &[u8]) -> Result<(), Error>;
    /// List all key-value pairs with a given prefix.
    async fn list_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Error>;
}

/// In-memory KV store for testing and ephemeral use.
pub struct InMemoryKvStore {
    data: RwLock<BTreeMap<Vec<u8>, Vec<u8>>>,
}

impl InMemoryKvStore {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(BTreeMap::new()),
        }
    }
}

impl Default for InMemoryKvStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl KvStore for InMemoryKvStore {
    async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        let data = self
            .data
            .read()
            .map_err(|e| Error::InvalidData(format!("lock poisoned: {e}")))?;
        Ok(data.get(key).cloned())
    }

    async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), Error> {
        let mut data = self
            .data
            .write()
            .map_err(|e| Error::InvalidData(format!("lock poisoned: {e}")))?;
        data.insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    async fn delete(&self, key: &[u8]) -> Result<(), Error> {
        let mut data = self
            .data
            .write()
            .map_err(|e| Error::InvalidData(format!("lock poisoned: {e}")))?;
        data.remove(key);
        Ok(())
    }

    async fn list_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Error> {
        let data = self
            .data
            .read()
            .map_err(|e| Error::InvalidData(format!("lock poisoned: {e}")))?;
        let results = data
            .range(prefix.to_vec()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_get_delete() {
        let store = InMemoryKvStore::new();
        assert!(store.get(b"key1").await.unwrap().is_none());

        store.put(b"key1", b"value1").await.unwrap();
        assert_eq!(store.get(b"key1").await.unwrap().unwrap(), b"value1");

        store.delete(b"key1").await.unwrap();
        assert!(store.get(b"key1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_prefix_filtering() {
        let store = InMemoryKvStore::new();
        store.put(b"contract:a", b"1").await.unwrap();
        store.put(b"contract:b", b"2").await.unwrap();
        store.put(b"key:x", b"3").await.unwrap();

        let results = store.list_prefix(b"contract:").await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, b"contract:a");
        assert_eq!(results[1].0, b"contract:b");
    }
}
