//! Note storage for discovered private notes.
//!
//! Phase 2 enhanced version with scope support, nullification block tracking,
//! status filtering, rollback, and batch operations matching the TS NoteStore.

use std::sync::Arc;

use aztec_core::error::Error;
use aztec_core::types::{AztecAddress, Fr};
use serde::{Deserialize, Serialize};

use super::kv::KvStore;

/// Note status for filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoteStatus {
    /// Note is active (not nullified).
    Active,
    /// Note has been nullified.
    Nullified,
    /// Match both active and nullified notes.
    ActiveOrNullified,
}

impl Default for NoteStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// A discovered private note.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredNote {
    /// The contract that owns this note.
    pub contract_address: AztecAddress,
    /// The storage slot within the contract.
    pub storage_slot: Fr,
    /// The note hash (commitment).
    pub note_hash: Fr,
    /// The siloed nullifier for this note.
    pub siloed_nullifier: Fr,
    /// The note's field data.
    pub note_data: Vec<Fr>,
    /// Whether this note has been nullified.
    pub nullified: bool,
    /// Block number when this note was nullified (if nullified).
    pub nullification_block_number: Option<u64>,
    /// Index in the note hash tree (if known).
    pub leaf_index: Option<u64>,
    /// Block number when this note was created.
    pub block_number: Option<u64>,
    /// Index of the transaction within the block.
    pub tx_index_in_block: Option<u64>,
    /// Index of the note within the transaction.
    pub note_index_in_tx: Option<u64>,
    /// Scopes (accounts) that can access this note.
    pub scopes: Vec<AztecAddress>,
}

/// Filter for querying notes.
#[derive(Debug, Clone, Default)]
pub struct NoteFilter {
    /// Filter by contract address (required in TS but optional here for flexibility).
    pub contract_address: Option<AztecAddress>,
    /// Filter by storage slot.
    pub storage_slot: Option<Fr>,
    /// Filter by owner/scope address.
    pub owner: Option<AztecAddress>,
    /// Filter by note status.
    pub status: NoteStatus,
    /// Filter by specific scopes.
    pub scopes: Vec<AztecAddress>,
    /// Filter by siloed nullifier.
    pub siloed_nullifier: Option<Fr>,
}

/// Stores discovered notes indexed by siloed nullifier (unique key).
///
/// Phase 2 enhanced version with:
/// - Scope-based access control
/// - Nullification with block number tracking
/// - Status filtering (Active/Nullified/Both)
/// - Rollback support for chain reorgs
/// - Batch note addition
pub struct NoteStore {
    kv: Arc<dyn KvStore>,
}

impl NoteStore {
    pub fn new(kv: Arc<dyn KvStore>) -> Self {
        Self { kv }
    }

    /// Add multiple notes at once.
    pub async fn add_notes(&self, notes: &[StoredNote], scope: &AztecAddress) -> Result<(), Error> {
        for note in notes {
            let mut stored = note.clone();
            // Add scope if not already present
            if !stored.scopes.contains(scope) {
                stored.scopes.push(*scope);
            }

            let key = note_key_by_nullifier(&stored.siloed_nullifier);

            // Check if note already exists (might need scope merge)
            if let Some(existing_bytes) = self.kv.get(&key).await? {
                let mut existing: StoredNote = serde_json::from_slice(&existing_bytes)?;
                if !existing.scopes.contains(scope) {
                    existing.scopes.push(*scope);
                }
                let value = serde_json::to_vec(&existing)?;
                self.kv.put(&key, &value).await?;
            } else {
                let value = serde_json::to_vec(&stored)?;
                self.kv.put(&key, &value).await?;

                // Index by contract address
                self.add_to_contract_index(&stored.contract_address, &stored.siloed_nullifier)
                    .await?;

                // Index by block number if known
                if let Some(bn) = stored.block_number {
                    self.add_to_block_index(bn, &stored.siloed_nullifier)
                        .await?;
                }
            }
        }
        Ok(())
    }

    /// Add a single discovered note (backward compatible).
    pub async fn add_note(&self, note: &StoredNote) -> Result<(), Error> {
        let scope = note.scopes.first().copied().unwrap_or(AztecAddress::zero());
        self.add_notes(&[note.clone()], &scope).await
    }

    /// Get notes matching a filter.
    pub async fn get_notes(&self, filter: &NoteFilter) -> Result<Vec<StoredNote>, Error> {
        let notes = if let Some(ref contract) = filter.contract_address {
            self.get_notes_for_contract(contract).await?
        } else {
            self.get_all_notes().await?
        };

        let filtered: Vec<StoredNote> = notes
            .into_iter()
            .filter(|note| {
                // Status filter
                match filter.status {
                    NoteStatus::Active => {
                        if note.nullified {
                            return false;
                        }
                    }
                    NoteStatus::Nullified => {
                        if !note.nullified {
                            return false;
                        }
                    }
                    NoteStatus::ActiveOrNullified => {}
                }

                // Storage slot filter
                if let Some(ref slot) = filter.storage_slot {
                    if note.storage_slot != *slot {
                        return false;
                    }
                }

                // Owner/scope filter
                if let Some(ref owner) = filter.owner {
                    if !note.scopes.contains(owner) {
                        return false;
                    }
                }

                // Scopes filter
                if !filter.scopes.is_empty()
                    && !note.scopes.iter().any(|s| filter.scopes.contains(s))
                {
                    return false;
                }

                // Siloed nullifier filter
                if let Some(ref nullifier) = filter.siloed_nullifier {
                    if note.siloed_nullifier != *nullifier {
                        return false;
                    }
                }

                true
            })
            .collect();

        // Sort by block_number, tx_index_in_block, note_index_in_tx
        let mut sorted = filtered;
        sorted.sort_by(|a, b| {
            a.block_number
                .cmp(&b.block_number)
                .then(a.tx_index_in_block.cmp(&b.tx_index_in_block))
                .then(a.note_index_in_tx.cmp(&b.note_index_in_tx))
        });

        Ok(sorted)
    }

    /// Get notes for a contract and storage slot (backward compatible).
    pub async fn get_notes_by_slot(
        &self,
        contract: &AztecAddress,
        storage_slot: &Fr,
    ) -> Result<Vec<StoredNote>, Error> {
        self.get_notes(&NoteFilter {
            contract_address: Some(*contract),
            storage_slot: Some(*storage_slot),
            status: NoteStatus::Active,
            ..Default::default()
        })
        .await
    }

    /// Apply nullifiers: mark notes as nullified with block number tracking.
    pub async fn apply_nullifiers(
        &self,
        nullifiers: &[(Fr, u64)], // (siloed_nullifier, block_number)
    ) -> Result<(), Error> {
        for (nullifier, block_number) in nullifiers {
            let key = note_key_by_nullifier(nullifier);
            if let Some(bytes) = self.kv.get(&key).await? {
                let mut note: StoredNote = serde_json::from_slice(&bytes)?;
                if !note.nullified {
                    note.nullified = true;
                    note.nullification_block_number = Some(*block_number);
                    let value = serde_json::to_vec(&note)?;
                    self.kv.put(&key, &value).await?;

                    // Index by nullification block
                    self.add_to_nullification_block_index(*block_number, nullifier)
                        .await?;
                }
            }
        }
        Ok(())
    }

    /// Mark a note as nullified (backward compatible).
    pub async fn nullify_note(
        &self,
        contract: &AztecAddress,
        storage_slot: &Fr,
        note_hash: &Fr,
    ) -> Result<(), Error> {
        // Search for the note by contract + storage_slot + note_hash
        let notes = self.get_notes_for_contract(contract).await?;
        for note in notes {
            if note.storage_slot == *storage_slot && note.note_hash == *note_hash {
                self.apply_nullifiers(&[(note.siloed_nullifier, 0)]).await?;
                return Ok(());
            }
        }
        Ok(())
    }

    /// Rollback: undo nullifications and delete notes after a given block.
    pub async fn rollback(
        &self,
        block_number: u64,
        _synced_block_number: u64,
    ) -> Result<(), Error> {
        // Phase 1: Un-nullify notes that were nullified after block_number
        let nullification_prefix = b"note_idx:nullify_block:";
        let entries = self.kv.list_prefix(nullification_prefix).await?;

        for (key, value) in &entries {
            let key_str = String::from_utf8_lossy(key);
            if let Some(bn_str) = key_str.strip_prefix("note_idx:nullify_block:") {
                if let Ok(bn) = bn_str.parse::<u64>() {
                    if bn > block_number {
                        let nullifiers: Vec<String> = serde_json::from_slice(value)?;
                        for nullifier_str in &nullifiers {
                            if let Ok(nullifier) = Fr::from_hex(nullifier_str) {
                                let note_key = note_key_by_nullifier(&nullifier);
                                if let Some(note_bytes) = self.kv.get(&note_key).await? {
                                    let mut note: StoredNote = serde_json::from_slice(&note_bytes)?;
                                    note.nullified = false;
                                    note.nullification_block_number = None;
                                    self.kv.put(&note_key, &serde_json::to_vec(&note)?).await?;
                                }
                            }
                        }
                        self.kv.delete(key).await?;
                    }
                }
            }
        }

        // Phase 2: Delete active notes created after block_number
        let block_prefix = b"note_idx:block:";
        let block_entries = self.kv.list_prefix(block_prefix).await?;

        for (key, value) in &block_entries {
            let key_str = String::from_utf8_lossy(key);
            if let Some(bn_str) = key_str.strip_prefix("note_idx:block:") {
                if let Ok(bn) = bn_str.parse::<u64>() {
                    if bn > block_number {
                        let nullifiers: Vec<String> = serde_json::from_slice(value)?;
                        for nullifier_str in &nullifiers {
                            if let Ok(nullifier) = Fr::from_hex(nullifier_str) {
                                let note_key = note_key_by_nullifier(&nullifier);
                                // Remove from contract index
                                if let Some(note_bytes) = self.kv.get(&note_key).await? {
                                    let note: StoredNote = serde_json::from_slice(&note_bytes)?;
                                    self.remove_from_contract_index(
                                        &note.contract_address,
                                        &nullifier,
                                    )
                                    .await?;
                                }
                                self.kv.delete(&note_key).await?;
                            }
                        }
                        self.kv.delete(key).await?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if a note hash exists in the store (active only).
    pub async fn has_note(&self, contract: &AztecAddress, note_hash: &Fr) -> Result<bool, Error> {
        let notes = self.get_notes_for_contract(contract).await?;
        Ok(notes
            .iter()
            .any(|n| n.note_hash == *note_hash && !n.nullified))
    }

    // --- Index management ---

    async fn get_notes_for_contract(
        &self,
        contract: &AztecAddress,
    ) -> Result<Vec<StoredNote>, Error> {
        let idx_key = contract_index_key(contract);
        let nullifiers: Vec<String> = match self.kv.get(&idx_key).await? {
            Some(bytes) => serde_json::from_slice(&bytes)?,
            None => return Ok(vec![]),
        };

        let mut notes = Vec::new();
        for nullifier_str in nullifiers {
            if let Ok(nullifier) = Fr::from_hex(&nullifier_str) {
                let key = note_key_by_nullifier(&nullifier);
                if let Some(bytes) = self.kv.get(&key).await? {
                    notes.push(serde_json::from_slice(&bytes)?);
                }
            }
        }
        Ok(notes)
    }

    async fn get_all_notes(&self) -> Result<Vec<StoredNote>, Error> {
        let prefix = b"note:";
        let entries = self.kv.list_prefix(prefix).await?;
        entries
            .into_iter()
            .map(|(_, v)| Ok(serde_json::from_slice(&v)?))
            .collect()
    }

    async fn add_to_contract_index(
        &self,
        contract: &AztecAddress,
        nullifier: &Fr,
    ) -> Result<(), Error> {
        let key = contract_index_key(contract);
        let mut list: Vec<String> = match self.kv.get(&key).await? {
            Some(bytes) => serde_json::from_slice(&bytes)?,
            None => vec![],
        };
        let nullifier_str = format!("{nullifier}");
        if !list.contains(&nullifier_str) {
            list.push(nullifier_str);
            self.kv.put(&key, &serde_json::to_vec(&list)?).await?;
        }
        Ok(())
    }

    async fn remove_from_contract_index(
        &self,
        contract: &AztecAddress,
        nullifier: &Fr,
    ) -> Result<(), Error> {
        let key = contract_index_key(contract);
        if let Some(bytes) = self.kv.get(&key).await? {
            let mut list: Vec<String> = serde_json::from_slice(&bytes)?;
            let nullifier_str = format!("{nullifier}");
            list.retain(|s| s != &nullifier_str);
            if list.is_empty() {
                self.kv.delete(&key).await?;
            } else {
                self.kv.put(&key, &serde_json::to_vec(&list)?).await?;
            }
        }
        Ok(())
    }

    async fn add_to_block_index(&self, block_number: u64, nullifier: &Fr) -> Result<(), Error> {
        let key = block_index_key(block_number);
        let mut list: Vec<String> = match self.kv.get(&key).await? {
            Some(bytes) => serde_json::from_slice(&bytes)?,
            None => vec![],
        };
        let nullifier_str = format!("{nullifier}");
        if !list.contains(&nullifier_str) {
            list.push(nullifier_str);
            self.kv.put(&key, &serde_json::to_vec(&list)?).await?;
        }
        Ok(())
    }

    async fn add_to_nullification_block_index(
        &self,
        block_number: u64,
        nullifier: &Fr,
    ) -> Result<(), Error> {
        let key = nullification_block_index_key(block_number);
        let mut list: Vec<String> = match self.kv.get(&key).await? {
            Some(bytes) => serde_json::from_slice(&bytes)?,
            None => vec![],
        };
        let nullifier_str = format!("{nullifier}");
        if !list.contains(&nullifier_str) {
            list.push(nullifier_str);
            self.kv.put(&key, &serde_json::to_vec(&list)?).await?;
        }
        Ok(())
    }
}

fn note_key_by_nullifier(nullifier: &Fr) -> Vec<u8> {
    format!("note:{nullifier}").into_bytes()
}

fn contract_index_key(contract: &AztecAddress) -> Vec<u8> {
    format!("note_idx:contract:{contract}").into_bytes()
}

fn block_index_key(block_number: u64) -> Vec<u8> {
    format!("note_idx:block:{block_number}").into_bytes()
}

fn nullification_block_index_key(block_number: u64) -> Vec<u8> {
    format!("note_idx:nullify_block:{block_number}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stores::InMemoryKvStore;

    fn make_note(contract: u64, slot: u64, hash: u64, nullifier: u64) -> StoredNote {
        StoredNote {
            contract_address: AztecAddress::from(contract),
            storage_slot: Fr::from(slot),
            note_hash: Fr::from(hash),
            siloed_nullifier: Fr::from(nullifier),
            note_data: vec![Fr::from(10u64), Fr::from(20u64)],
            nullified: false,
            nullification_block_number: None,
            leaf_index: None,
            block_number: Some(1),
            tx_index_in_block: Some(0),
            note_index_in_tx: Some(0),
            scopes: vec![],
        }
    }

    #[tokio::test]
    async fn add_and_get_notes() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = NoteStore::new(kv);
        let scope = AztecAddress::from(99u64);
        let note = make_note(1, 5, 100, 200);

        store.add_notes(&[note], &scope).await.unwrap();

        let notes = store
            .get_notes(&NoteFilter {
                contract_address: Some(AztecAddress::from(1u64)),
                storage_slot: Some(Fr::from(5u64)),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].note_hash, Fr::from(100u64));
        assert!(notes[0].scopes.contains(&scope));
    }

    #[tokio::test]
    async fn apply_nullifiers_and_filter() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = NoteStore::new(kv);
        let scope = AztecAddress::from(99u64);
        let note = make_note(1, 5, 100, 200);

        store.add_notes(&[note], &scope).await.unwrap();
        store
            .apply_nullifiers(&[(Fr::from(200u64), 5)])
            .await
            .unwrap();

        // Active filter returns empty
        let active = store
            .get_notes(&NoteFilter {
                contract_address: Some(AztecAddress::from(1u64)),
                status: NoteStatus::Active,
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(active.is_empty());

        // Nullified filter returns the note
        let nullified = store
            .get_notes(&NoteFilter {
                contract_address: Some(AztecAddress::from(1u64)),
                status: NoteStatus::Nullified,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(nullified.len(), 1);
        assert_eq!(nullified[0].nullification_block_number, Some(5));
    }

    #[tokio::test]
    async fn rollback_un_nullifies_notes() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = NoteStore::new(kv);
        let scope = AztecAddress::from(99u64);
        let note = make_note(1, 5, 100, 200);

        store.add_notes(&[note], &scope).await.unwrap();
        store
            .apply_nullifiers(&[(Fr::from(200u64), 10)])
            .await
            .unwrap();

        // Rollback to block 5 should un-nullify
        store.rollback(5, 5).await.unwrap();

        let active = store
            .get_notes(&NoteFilter {
                contract_address: Some(AztecAddress::from(1u64)),
                status: NoteStatus::Active,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert!(!active[0].nullified);
    }

    #[tokio::test]
    async fn scope_filtering() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = NoteStore::new(kv);
        let scope1 = AztecAddress::from(1u64);
        let scope2 = AztecAddress::from(2u64);
        let note = make_note(10, 5, 100, 200);

        store.add_notes(&[note], &scope1).await.unwrap();

        // scope1 can see it
        let notes = store
            .get_notes(&NoteFilter {
                contract_address: Some(AztecAddress::from(10u64)),
                scopes: vec![scope1],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(notes.len(), 1);

        // scope2 cannot
        let notes = store
            .get_notes(&NoteFilter {
                contract_address: Some(AztecAddress::from(10u64)),
                scopes: vec![scope2],
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(notes.is_empty());
    }

    #[tokio::test]
    async fn backward_compat_add_and_nullify() {
        let kv = Arc::new(InMemoryKvStore::new());
        let store = NoteStore::new(kv);
        let note = make_note(1, 5, 100, 200);

        store.add_note(&note).await.unwrap();
        let found = store
            .has_note(&AztecAddress::from(1u64), &Fr::from(100u64))
            .await
            .unwrap();
        assert!(found);

        store
            .nullify_note(
                &AztecAddress::from(1u64),
                &Fr::from(5u64),
                &Fr::from(100u64),
            )
            .await
            .unwrap();

        let found = store
            .has_note(&AztecAddress::from(1u64), &Fr::from(100u64))
            .await
            .unwrap();
        assert!(!found);
    }
}
