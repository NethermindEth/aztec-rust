//! Anchor block store for persisting the current synced block header.
//!
//! Ports the TS `AnchorBlockStore` which stores the current synchronized
//! block header. Updated only from the BlockStateSynchronizer.

use std::sync::Arc;

use aztec_core::error::Error;
use serde::{Deserialize, Serialize};

use super::kv::KvStore;

/// Key used to store the anchor block header.
const ANCHOR_HEADER_KEY: &[u8] = b"anchor:header";

/// Stored anchor block header with extracted metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorBlockHeader {
    /// The full block header data (opaque JSON matching TS types).
    pub data: serde_json::Value,
    /// Extracted block number for convenience.
    pub block_number: u64,
    /// Extracted block hash for convenience.
    pub block_hash: String,
}

impl AnchorBlockHeader {
    /// Extract block number from the header JSON.
    pub fn get_block_number(&self) -> u64 {
        self.block_number
    }

    /// Extract block hash from the header JSON.
    pub fn get_block_hash(&self) -> &str {
        &self.block_hash
    }

    /// Create from raw header JSON, extracting metadata.
    pub fn from_header_json(data: serde_json::Value) -> Self {
        let block_number = data
            .pointer("/globalVariables/blockNumber")
            .and_then(|v| {
                v.as_u64().or_else(|| {
                    v.as_str().and_then(|s| {
                        u64::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16).ok()
                    })
                })
            })
            .unwrap_or(0);

        let block_hash = data
            .pointer("/blockHash")
            .or_else(|| data.get("blockHash"))
            .and_then(|v| v.as_str())
            .unwrap_or("0x0")
            .to_owned();

        Self {
            data,
            block_number,
            block_hash,
        }
    }
}

/// Stores the current anchor (synced) block header.
///
/// The anchor block header determines which state the PXE considers current.
/// It is updated by the BlockStateSynchronizer and read during transaction
/// simulation and event retrieval.
pub struct AnchorBlockStore {
    kv: Arc<dyn KvStore>,
}

impl AnchorBlockStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    /// Set the anchor block header.
    pub async fn set_header(&self, header: &AnchorBlockHeader) -> Result<(), Error> {
        let value = serde_json::to_vec(header)?;
        self.kv.put(ANCHOR_HEADER_KEY, &value).await
    }

    /// Get the anchor block header.
    ///
    /// Returns `None` if no header has been set yet.
    pub async fn get_block_header(&self) -> Result<Option<AnchorBlockHeader>, Error> {
        match self.kv.get(ANCHOR_HEADER_KEY).await? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            None => Ok(None),
        }
    }

    /// Get the anchor block header, returning an error if not set.
    pub async fn get_block_header_or_err(&self) -> Result<AnchorBlockHeader, Error> {
        self.get_block_header()
            .await?
            .ok_or_else(|| Error::InvalidData("anchor block header not set".into()))
    }

    /// Get the current anchor block number, or 0 if not set.
    pub async fn get_block_number(&self) -> Result<u64, Error> {
        match self.get_block_header().await? {
            Some(header) => Ok(header.block_number),
            None => Ok(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::InMemoryKvStore;

    #[tokio::test]
    async fn set_and_get_header() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = AnchorBlockStore::new(kv);

        let header = AnchorBlockHeader::from_header_json(serde_json::json!({
            "globalVariables": {"blockNumber": 42},
            "blockHash": "0xabc123"
        }));

        store.set_header(&header).await.unwrap();
        let retrieved = store.get_block_header().await.unwrap().unwrap();
        assert_eq!(retrieved.block_number, 42);
        assert_eq!(retrieved.block_hash, "0xabc123");
    }

    #[tokio::test]
    async fn get_header_returns_none_when_unset() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = AnchorBlockStore::new(kv);
        assert!(store.get_block_header().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn get_header_or_err_fails_when_unset() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = AnchorBlockStore::new(kv);
        assert!(store.get_block_header_or_err().await.is_err());
    }

    #[tokio::test]
    async fn from_header_json_extracts_block_number() {
        let header = AnchorBlockHeader::from_header_json(serde_json::json!({
            "globalVariables": {"blockNumber": 100}
        }));
        assert_eq!(header.block_number, 100);
    }

    #[tokio::test]
    async fn from_header_json_handles_hex_block_number() {
        let header = AnchorBlockHeader::from_header_json(serde_json::json!({
            "globalVariables": {"blockNumber": "0x0a"}
        }));
        assert_eq!(header.block_number, 10);
    }

    #[tokio::test]
    async fn set_header_overwrites_previous() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = AnchorBlockStore::new(kv);

        let h1 = AnchorBlockHeader::from_header_json(serde_json::json!({
            "globalVariables": {"blockNumber": 1}
        }));
        let h2 = AnchorBlockHeader::from_header_json(serde_json::json!({
            "globalVariables": {"blockNumber": 2}
        }));

        store.set_header(&h1).await.unwrap();
        store.set_header(&h2).await.unwrap();

        let retrieved = store.get_block_header().await.unwrap().unwrap();
        assert_eq!(retrieved.block_number, 2);
    }
}
