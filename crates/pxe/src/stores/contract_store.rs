//! Contract artifact and instance storage.

use std::sync::Arc;

use aztec_core::abi::ContractArtifact;
use aztec_core::error::Error;
use aztec_core::types::{AztecAddress, ContractInstanceWithAddress, Fr};

use super::kv::KvStore;

/// Stores contract artifacts, instances, and class registrations.
pub struct ContractStore {
    kv: Arc<dyn KvStore>,
}

impl ContractStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    // --- Contract Instances ---

    /// Store a contract instance by its address.
    pub async fn add_instance(&self, instance: &ContractInstanceWithAddress) -> Result<(), Error> {
        let key = instance_key(&instance.address);
        let value = serde_json::to_vec(instance)?;
        self.kv.put(&key, &value).await
    }

    /// Get a contract instance by address.
    pub async fn get_instance(
        &self,
        address: &AztecAddress,
    ) -> Result<Option<ContractInstanceWithAddress>, Error> {
        let key = instance_key(address);
        match self.kv.get(&key).await? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            None => Ok(None),
        }
    }

    /// List all registered contract addresses.
    pub async fn get_contract_addresses(&self) -> Result<Vec<AztecAddress>, Error> {
        let entries = self.kv.list_prefix(b"contract:instance:").await?;
        entries
            .into_iter()
            .map(|(_, v)| {
                let inst: ContractInstanceWithAddress = serde_json::from_slice(&v)?;
                Ok(inst.address)
            })
            .collect()
    }

    // --- Contract Artifacts ---

    /// Store a contract artifact by class ID.
    pub async fn add_artifact(
        &self,
        class_id: &Fr,
        artifact: &ContractArtifact,
    ) -> Result<(), Error> {
        let key = artifact_key(class_id);
        let value = serde_json::to_vec(artifact)?;
        self.kv.put(&key, &value).await
    }

    /// Get a contract artifact by class ID.
    pub async fn get_artifact(&self, class_id: &Fr) -> Result<Option<ContractArtifact>, Error> {
        let key = artifact_key(class_id);
        match self.kv.get(&key).await? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            None => Ok(None),
        }
    }

    /// Update a contract's artifact (by address — looks up the class ID).
    pub async fn update_artifact(
        &self,
        address: &AztecAddress,
        artifact: &ContractArtifact,
    ) -> Result<(), Error> {
        let instance = self
            .get_instance(address)
            .await?
            .ok_or_else(|| Error::InvalidData(format!("contract not found at {address}")))?;
        self.add_artifact(&instance.inner.current_contract_class_id, artifact)
            .await
    }

    // --- Contract Classes ---

    /// Register a contract class (stores the artifact keyed by computed class ID).
    pub async fn add_class(&self, artifact: &ContractArtifact) -> Result<Fr, Error> {
        let class_id = aztec_core::hash::compute_contract_class_id_from_artifact(artifact)?;
        self.add_artifact(&class_id, artifact).await?;
        Ok(class_id)
    }
}

fn instance_key(address: &AztecAddress) -> Vec<u8> {
    format!("contract:instance:{address}").into_bytes()
}

fn artifact_key(class_id: &Fr) -> Vec<u8> {
    format!("contract:artifact:{class_id}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::InMemoryKvStore;
    use aztec_core::types::ContractInstance;

    fn test_instance() -> ContractInstanceWithAddress {
        ContractInstanceWithAddress {
            address: AztecAddress::from(42u64),
            inner: ContractInstance {
                version: 1,
                salt: Fr::from(1u64),
                deployer: AztecAddress::zero(),
                current_contract_class_id: Fr::from(100u64),
                original_contract_class_id: Fr::from(100u64),
                initialization_hash: Fr::zero(),
                public_keys: Default::default(),
            },
        }
    }

    #[tokio::test]
    async fn store_and_retrieve_instance() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = ContractStore::new(kv);
        let inst = test_instance();

        store.add_instance(&inst).await.unwrap();
        let retrieved = store.get_instance(&inst.address).await.unwrap().unwrap();
        assert_eq!(retrieved.address, inst.address);
    }

    #[tokio::test]
    async fn list_contracts() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = ContractStore::new(kv);

        assert!(store.get_contract_addresses().await.unwrap().is_empty());

        store.add_instance(&test_instance()).await.unwrap();
        let addrs = store.get_contract_addresses().await.unwrap();
        assert_eq!(addrs.len(), 1);
    }
}
