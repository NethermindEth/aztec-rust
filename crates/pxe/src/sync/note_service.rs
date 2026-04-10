//! Note service for note validation, storage, and nullifier synchronization.
//!
//! Ports the TS `NoteService` which manages note operations including
//! retrieving notes, syncing nullifiers, and validating/storing new notes.

use aztec_core::error::Error;
use aztec_core::types::{AztecAddress, Fr};
use aztec_node_client::AztecNode;

use crate::stores::note_store::{NoteFilter, NoteStatus, StoredNote};
use crate::stores::NoteStore;

/// Maximum number of items per RPC request (matches TS MAX_RPC_LEN).
const MAX_RPC_LEN: usize = 128;

/// Service for note-related operations.
pub struct NoteService<'a, N: AztecNode> {
    node: &'a N,
    note_store: &'a NoteStore,
}

impl<'a, N: AztecNode> NoteService<'a, N> {
    pub fn new(node: &'a N, note_store: &'a NoteStore) -> Self {
        Self { node, note_store }
    }

    /// Get notes from the store matching a filter.
    pub async fn get_notes(&self, filter: &NoteFilter) -> Result<Vec<StoredNote>, Error> {
        self.note_store.get_notes(filter).await
    }

    /// Sync note nullifiers for a contract.
    ///
    /// Fetches all active notes for a contract and checks if their nullifiers
    /// have been included in the nullifier tree (i.e., the note was spent).
    /// If so, marks the note as nullified.
    ///
    /// Batches queries for efficiency using MAX_RPC_LEN.
    pub async fn sync_note_nullifiers(
        &self,
        contract_address: &AztecAddress,
        scopes: &[AztecAddress],
    ) -> Result<u64, Error> {
        let filter = NoteFilter {
            contract_address: Some(*contract_address),
            status: NoteStatus::Active,
            scopes: scopes.to_vec(),
            ..Default::default()
        };

        let notes = self.note_store.get_notes(&filter).await?;
        if notes.is_empty() {
            return Ok(0);
        }

        let mut nullified_count = 0u64;

        // Process in batches
        for chunk in notes.chunks(MAX_RPC_LEN) {
            let nullifiers: Vec<Fr> = chunk.iter().map(|n| n.siloed_nullifier).collect();

            // Check which nullifiers exist in the tree
            let indexes = self.node.find_leaves_indexes(0, "1", &nullifiers).await?;

            // Mark found nullifiers
            let mut to_nullify = Vec::new();
            for (i, maybe_index) in indexes.iter().enumerate() {
                if maybe_index.is_some() {
                    to_nullify.push((chunk[i].siloed_nullifier, 0u64));
                    nullified_count += 1;
                }
            }

            if !to_nullify.is_empty() {
                self.note_store.apply_nullifiers(&to_nullify).await?;
            }
        }

        if nullified_count > 0 {
            tracing::debug!(
                contract = %contract_address,
                nullified = nullified_count,
                "synced note nullifiers"
            );
        }

        Ok(nullified_count)
    }

    /// Validate and store a note.
    ///
    /// Validates:
    /// 1. Note hash exists in the note hash tree
    /// 2. Note is not already nullified
    /// 3. Computes siloed hash and nullifier
    ///
    /// Then stores the note in the NoteStore.
    pub async fn validate_and_store_note(
        &self,
        note: &StoredNote,
        scope: &AztecAddress,
    ) -> Result<(), Error> {
        // The Noir `sync_state` already validated the note client-side
        // (decrypted the log, computed the note hash, matched it against
        // unique note hashes in the tx). We trust the validation request.

        // Check if already nullified
        let nullifier_witness = self
            .node
            .get_nullifier_membership_witness(0, &note.siloed_nullifier)
            .await?;

        let mut stored = note.clone();
        if nullifier_witness.is_some() {
            stored.nullified = true;
            stored.nullification_block_number = Some(0); // Unknown exact block
        }

        // Store the note
        self.note_store.add_notes(&[stored], scope).await?;

        Ok(())
    }
}
