//! Ephemeral capsule storage for private execution.

use std::sync::Arc;

use aztec_core::error::Error;
use aztec_core::types::{AztecAddress, Fr};

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

    /// Store capsule data in a contract-scoped slot.
    pub async fn store_capsule(
        &self,
        contract_address: &AztecAddress,
        slot: &Fr,
        capsule: &[Fr],
    ) -> Result<(), Error> {
        let key = db_slot_key(contract_address, slot);
        let value = serde_json::to_vec(capsule)?;
        self.kv.put(&key, &value).await
    }

    /// Load capsule data from a contract-scoped slot.
    pub async fn load_capsule(
        &self,
        contract_address: &AztecAddress,
        slot: &Fr,
    ) -> Result<Option<Vec<Fr>>, Error> {
        let key = db_slot_key(contract_address, slot);
        match self.kv.get(&key).await? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            None => Ok(None),
        }
    }

    /// Delete capsule data from a contract-scoped slot.
    pub async fn delete_capsule(
        &self,
        contract_address: &AztecAddress,
        slot: &Fr,
    ) -> Result<(), Error> {
        let key = db_slot_key(contract_address, slot);
        self.kv.delete(&key).await
    }

    /// Copy a contiguous region of contract-scoped slots.
    pub async fn copy_capsule(
        &self,
        contract_address: &AztecAddress,
        src_slot: &Fr,
        dst_slot: &Fr,
        num_entries: usize,
    ) -> Result<(), Error> {
        if num_entries == 0 {
            return Ok(());
        }

        let mut copied = Vec::with_capacity(num_entries);
        for i in 0..num_entries {
            let current_src = Fr::from((src_slot.to_usize() + i) as u64);
            let data = self
                .load_capsule(contract_address, &current_src)
                .await?
                .ok_or_else(|| {
                    Error::InvalidData(format!(
                        "attempted to copy empty capsule slot {} for contract {}",
                        current_src, contract_address
                    ))
                })?;
            copied.push(data);
        }

        for (i, data) in copied.into_iter().enumerate() {
            let current_dst = Fr::from((dst_slot.to_usize() + i) as u64);
            self.store_capsule(contract_address, &current_dst, &data)
                .await?;
        }

        Ok(())
    }

    /// Append entries to a capsule array stored at `base_slot`.
    pub async fn append_to_capsule_array(
        &self,
        contract_address: &AztecAddress,
        base_slot: &Fr,
        content: &[Vec<Fr>],
    ) -> Result<(), Error> {
        let current_length = self
            .load_capsule(contract_address, base_slot)
            .await?
            .and_then(|capsule| capsule.first().copied())
            .unwrap_or_else(Fr::zero)
            .to_usize();

        for (i, capsule) in content.iter().enumerate() {
            let next_slot = array_slot(base_slot, current_length + i);
            self.store_capsule(contract_address, &next_slot, capsule)
                .await?;
        }

        self.store_capsule(
            contract_address,
            base_slot,
            &[Fr::from((current_length + content.len()) as u64)],
        )
        .await
    }

    /// Read all entries from a capsule array stored at `base_slot`.
    pub async fn read_capsule_array(
        &self,
        contract_address: &AztecAddress,
        base_slot: &Fr,
    ) -> Result<Vec<Vec<Fr>>, Error> {
        let length = self
            .load_capsule(contract_address, base_slot)
            .await?
            .and_then(|capsule| capsule.first().copied())
            .unwrap_or_else(Fr::zero)
            .to_usize();

        let mut values = Vec::with_capacity(length);
        for i in 0..length {
            let slot = array_slot(base_slot, i);
            let value = self
                .load_capsule(contract_address, &slot)
                .await?
                .ok_or_else(|| {
                    Error::InvalidData(format!(
                        "expected non-empty capsule array value at slot {} for contract {}",
                        slot, contract_address
                    ))
                })?;
            values.push(value);
        }

        Ok(values)
    }

    /// Replace the entire capsule array stored at `base_slot`.
    pub async fn set_capsule_array(
        &self,
        contract_address: &AztecAddress,
        base_slot: &Fr,
        content: &[Vec<Fr>],
    ) -> Result<(), Error> {
        let original_length = self
            .load_capsule(contract_address, base_slot)
            .await?
            .and_then(|capsule| capsule.first().copied())
            .unwrap_or_else(Fr::zero)
            .to_usize();

        self.store_capsule(
            contract_address,
            base_slot,
            &[Fr::from(content.len() as u64)],
        )
        .await?;

        for (i, capsule) in content.iter().enumerate() {
            let slot = array_slot(base_slot, i);
            self.store_capsule(contract_address, &slot, capsule).await?;
        }

        for i in content.len()..original_length {
            let slot = array_slot(base_slot, i);
            self.delete_capsule(contract_address, &slot).await?;
        }

        Ok(())
    }
}

fn capsule_key(contract: &Fr) -> Vec<u8> {
    format!("capsule:{contract}").into_bytes()
}

fn db_slot_key(contract_address: &AztecAddress, slot: &Fr) -> Vec<u8> {
    format!("capsule_db:{contract_address}:{slot}").into_bytes()
}

fn array_slot(base_slot: &Fr, index: usize) -> Fr {
    Fr::from((base_slot.to_usize() + 1 + index) as u64)
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

    #[tokio::test]
    async fn contract_scoped_capsule_array_roundtrip() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = CapsuleStore::new(kv);
        let contract = AztecAddress::from(1u64);
        let base_slot = Fr::from(10u64);

        store
            .append_to_capsule_array(
                &contract,
                &base_slot,
                &[
                    vec![Fr::from(11u64)],
                    vec![Fr::from(12u64), Fr::from(13u64)],
                ],
            )
            .await
            .unwrap();

        let values = store
            .read_capsule_array(&contract, &base_slot)
            .await
            .unwrap();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0], vec![Fr::from(11u64)]);
        assert_eq!(values[1], vec![Fr::from(12u64), Fr::from(13u64)]);

        store
            .set_capsule_array(&contract, &base_slot, &[vec![Fr::from(99u64)]])
            .await
            .unwrap();

        let values = store
            .read_capsule_array(&contract, &base_slot)
            .await
            .unwrap();
        assert_eq!(values, vec![vec![Fr::from(99u64)]]);
    }
}
