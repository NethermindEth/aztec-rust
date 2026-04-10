//! Complete address storage.

use std::sync::Arc;

use aztec_core::error::Error;
use aztec_core::types::{AztecAddress, CompleteAddress};

use super::kv::KvStore;

/// Stores `CompleteAddress` records for registered accounts.
pub struct AddressStore {
    kv: Arc<dyn KvStore>,
}

impl AddressStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    /// Store a complete address.
    pub async fn add(&self, complete: &CompleteAddress) -> Result<(), Error> {
        let key = address_key(&complete.address);
        let value = serde_json::to_vec(complete)?;
        self.kv.put(&key, &value).await
    }

    /// Get a complete address by its Aztec address.
    pub async fn get(&self, address: &AztecAddress) -> Result<Option<CompleteAddress>, Error> {
        let key = address_key(address);
        match self.kv.get(&key).await? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            None => Ok(None),
        }
    }

    /// List all registered complete addresses.
    pub async fn get_all(&self) -> Result<Vec<CompleteAddress>, Error> {
        let entries = self.kv.list_prefix(b"address:").await?;
        entries
            .into_iter()
            .map(|(_, v)| Ok(serde_json::from_slice(&v)?))
            .collect()
    }
}

fn address_key(address: &AztecAddress) -> Vec<u8> {
    format!("address:{address}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::InMemoryKvStore;
    use aztec_core::types::{Fr, PublicKeys};

    #[tokio::test]
    async fn store_and_retrieve_address() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = AddressStore::new(kv);
        let complete = CompleteAddress {
            address: AztecAddress::from(42u64),
            public_keys: PublicKeys::default(),
            partial_address: Fr::from(1u64),
        };

        store.add(&complete).await.unwrap();
        let retrieved = store.get(&complete.address).await.unwrap().unwrap();
        assert_eq!(retrieved.address, complete.address);
    }
}
