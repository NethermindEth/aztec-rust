//! Note storage for discovered private notes.

use std::sync::Arc;

use aztec_core::error::Error;
use aztec_core::types::{AztecAddress, Fr};
use serde::{Deserialize, Serialize};

use super::kv::KvStore;

/// A discovered private note.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredNote {
    /// The contract that owns this note.
    pub contract_address: AztecAddress,
    /// The storage slot within the contract.
    pub storage_slot: Fr,
    /// The note hash (commitment).
    pub note_hash: Fr,
    /// The note's field data.
    pub note_data: Vec<Fr>,
    /// The note's nullifier (if computed).
    pub nullifier: Option<Fr>,
    /// Whether this note has been nullified.
    pub nullified: bool,
    /// Index in the note hash tree (if known).
    pub leaf_index: Option<u64>,
}

/// Stores discovered notes indexed by (contract, storage_slot).
pub struct NoteStore {
    kv: Arc<dyn KvStore>,
}

impl NoteStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    /// Add a discovered note.
    pub async fn add_note(&self, note: &StoredNote) -> Result<(), Error> {
        let key = note_key(&note.contract_address, &note.storage_slot, &note.note_hash);
        let value = serde_json::to_vec(note)?;
        self.kv.put(&key, &value).await
    }

    /// Get notes for a contract and storage slot.
    pub async fn get_notes(
        &self,
        contract: &AztecAddress,
        storage_slot: &Fr,
    ) -> Result<Vec<StoredNote>, Error> {
        let prefix = note_prefix(contract, storage_slot);
        let entries = self.kv.list_prefix(&prefix).await?;
        entries
            .into_iter()
            .map(|(_, v)| Ok(serde_json::from_slice(&v)?))
            .collect::<Result<Vec<StoredNote>, Error>>()
            .map(|notes| notes.into_iter().filter(|n| !n.nullified).collect())
    }

    /// Mark a note as nullified.
    pub async fn nullify_note(
        &self,
        contract: &AztecAddress,
        storage_slot: &Fr,
        note_hash: &Fr,
    ) -> Result<(), Error> {
        let key = note_key(contract, storage_slot, note_hash);
        if let Some(bytes) = self.kv.get(&key).await? {
            let mut note: StoredNote = serde_json::from_slice(&bytes)?;
            note.nullified = true;
            let value = serde_json::to_vec(&note)?;
            self.kv.put(&key, &value).await?;
        }
        Ok(())
    }

    /// Check if a note hash exists in the store.
    pub async fn has_note(&self, contract: &AztecAddress, note_hash: &Fr) -> Result<bool, Error> {
        // Search across all storage slots for this contract
        let prefix = format!("note:{}:", contract).into_bytes();
        let entries = self.kv.list_prefix(&prefix).await?;
        for (_, v) in entries {
            let note: StoredNote = serde_json::from_slice(&v)?;
            if note.note_hash == *note_hash && !note.nullified {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn note_prefix(contract: &AztecAddress, storage_slot: &Fr) -> Vec<u8> {
    format!("note:{contract}:{storage_slot}:").into_bytes()
}

fn note_key(contract: &AztecAddress, storage_slot: &Fr, note_hash: &Fr) -> Vec<u8> {
    format!("note:{contract}:{storage_slot}:{note_hash}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::InMemoryKvStore;

    #[tokio::test]
    async fn add_and_get_notes() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = NoteStore::new(kv);
        let contract = AztecAddress::from(1u64);
        let slot = Fr::from(5u64);

        let note = StoredNote {
            contract_address: contract,
            storage_slot: slot,
            note_hash: Fr::from(100u64),
            note_data: vec![Fr::from(10u64), Fr::from(20u64)],
            nullifier: None,
            nullified: false,
            leaf_index: None,
        };

        store.add_note(&note).await.unwrap();
        let notes = store.get_notes(&contract, &slot).await.unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].note_hash, Fr::from(100u64));
    }

    #[tokio::test]
    async fn nullify_filters_notes() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = NoteStore::new(kv);
        let contract = AztecAddress::from(1u64);
        let slot = Fr::from(5u64);
        let hash = Fr::from(100u64);

        let note = StoredNote {
            contract_address: contract,
            storage_slot: slot,
            note_hash: hash,
            note_data: vec![],
            nullifier: None,
            nullified: false,
            leaf_index: None,
        };

        store.add_note(&note).await.unwrap();
        store.nullify_note(&contract, &slot, &hash).await.unwrap();

        let notes = store.get_notes(&contract, &slot).await.unwrap();
        assert!(notes.is_empty());
    }
}
