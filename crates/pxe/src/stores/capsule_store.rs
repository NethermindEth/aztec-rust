//! Ephemeral capsule storage for private execution.

use std::sync::Arc;

use aztec_core::error::Error;
use aztec_core::types::Fr;

use super::kv::KvStore;

/// Stores ephemeral capsule data that is consumed during execution.
///
/// Capsules are private data blobs passed to contract functions via oracle calls.
/// They are consumed (deleted) after being read.
pub struct CapsuleStore {
    kv: Arc<dyn KvStore>,
}

impl CapsuleStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    /// Store a capsule (a list of field element arrays).
    pub async fn add(&self, contract: &Fr, capsule: &[Vec<Fr>]) -> Result<(), Error> {
        let key = capsule_key(contract);
        let value = serde_json::to_vec(capsule)?;
        self.kv.put(&key, &value).await
    }

    /// Pop a capsule for the given contract (consumes it).
    pub async fn pop(&self, contract: &Fr) -> Result<Option<Vec<Vec<Fr>>>, Error> {
        let key = capsule_key(contract);
        match self.kv.get(&key).await? {
            Some(bytes) => {
                self.kv.delete(&key).await?;
                Ok(Some(serde_json::from_slice(&bytes)?))
            }
            None => Ok(None),
        }
    }
}

fn capsule_key(contract: &Fr) -> Vec<u8> {
    format!("capsule:{contract}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::InMemoryKvStore;

    #[tokio::test]
    async fn capsule_is_consumed_on_pop() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = CapsuleStore::new(kv);
        let contract = Fr::from(1u64);
        let capsule = vec![vec![Fr::from(10u64), Fr::from(20u64)]];

        store.add(&contract, &capsule).await.unwrap();

        let first = store.pop(&contract).await.unwrap();
        assert!(first.is_some());

        let second = store.pop(&contract).await.unwrap();
        assert!(second.is_none());
    }
}
