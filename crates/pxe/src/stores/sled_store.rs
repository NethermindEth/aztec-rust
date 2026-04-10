//! Persistent KV store backed by sled — survives process restarts.
//!
//! Provides a `SledKvStore` that implements the `KvStore` trait using
//! the `sled` embedded database, matching Phase 3 requirement for
//! persistent storage.

use std::path::Path;

use async_trait::async_trait;
use aztec_core::error::Error;

use super::kv::KvStore;

/// Persistent key-value store backed by sled.
///
/// Stores data on disk in a directory specified at creation time.
/// Suitable for production use where PXE state must survive restarts.
pub struct SledKvStore {
    db: sled::Db,
}

impl SledKvStore {
    /// Open or create a sled database at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let db = sled::open(path.as_ref())
            .map_err(|e| Error::InvalidData(format!("failed to open sled database: {e}")))?;
        Ok(Self { db })
    }

    /// Open a temporary sled database (for testing).
    pub fn open_temporary() -> Result<Self, Error> {
        let config = sled::Config::new().temporary(true);
        let db = config.open().map_err(|e| {
            Error::InvalidData(format!("failed to open temporary sled database: {e}"))
        })?;
        Ok(Self { db })
    }

    /// Flush all pending writes to disk.
    pub fn flush(&self) -> Result<(), Error> {
        self.db
            .flush()
            .map_err(|e| Error::InvalidData(format!("sled flush failed: {e}")))?;
        Ok(())
    }

    /// Get the on-disk size estimate in bytes.
    pub fn size_on_disk(&self) -> Result<u64, Error> {
        self.db
            .size_on_disk()
            .map_err(|e| Error::InvalidData(format!("sled size_on_disk failed: {e}")))
    }
}

#[async_trait]
impl KvStore for SledKvStore {
    async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        self.db
            .get(key)
            .map(|opt| opt.map(|ivec| ivec.to_vec()))
            .map_err(|e| Error::InvalidData(format!("sled get failed: {e}")))
    }

    async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), Error> {
        self.db
            .insert(key, value)
            .map_err(|e| Error::InvalidData(format!("sled put failed: {e}")))?;
        Ok(())
    }

    async fn delete(&self, key: &[u8]) -> Result<(), Error> {
        self.db
            .remove(key)
            .map_err(|e| Error::InvalidData(format!("sled delete failed: {e}")))?;
        Ok(())
    }

    async fn list_prefix(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Error> {
        let results: Result<Vec<_>, _> = self
            .db
            .scan_prefix(prefix)
            .map(|result| {
                result
                    .map(|(k, v)| (k.to_vec(), v.to_vec()))
                    .map_err(|e| Error::InvalidData(format!("sled scan failed: {e}")))
            })
            .collect();
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> SledKvStore {
        SledKvStore::open_temporary().unwrap()
    }

    #[tokio::test]
    async fn put_get_delete() {
        let store = make_store();
        assert!(store.get(b"key1").await.unwrap().is_none());

        store.put(b"key1", b"value1").await.unwrap();
        assert_eq!(store.get(b"key1").await.unwrap().unwrap(), b"value1");

        store.delete(b"key1").await.unwrap();
        assert!(store.get(b"key1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_prefix_filtering() {
        let store = make_store();
        store.put(b"contract:a", b"1").await.unwrap();
        store.put(b"contract:b", b"2").await.unwrap();
        store.put(b"key:x", b"3").await.unwrap();

        let results = store.list_prefix(b"contract:").await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, b"contract:a");
        assert_eq!(results[1].0, b"contract:b");
    }

    #[tokio::test]
    async fn overwrite_value() {
        let store = make_store();
        store.put(b"k", b"v1").await.unwrap();
        store.put(b"k", b"v2").await.unwrap();
        assert_eq!(store.get(b"k").await.unwrap().unwrap(), b"v2");
    }

    #[tokio::test]
    async fn flush_succeeds() {
        let store = make_store();
        store.put(b"k", b"v").await.unwrap();
        store.flush().unwrap();
    }

    #[tokio::test]
    async fn size_on_disk_returns_value() {
        let store = make_store();
        let size = store.size_on_disk().unwrap();
        // Just verify it returns a value
        let _ = size;
    }
}
