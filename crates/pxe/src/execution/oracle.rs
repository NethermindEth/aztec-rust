//! Private execution oracle — bridges ACVM foreign calls to local stores + node RPC.

use aztec_core::error::Error;
use aztec_core::types::{AztecAddress, Fr};
use aztec_node_client::AztecNode;

use crate::stores::{CapsuleStore, ContractStore, KeyStore, NoteStore};

/// Oracle for private function execution.
///
/// Handles foreign-call callbacks from the ACVM during private function
/// execution, routing them to the appropriate local store or node RPC.
pub struct PrivateExecutionOracle<'a, N: AztecNode> {
    node: &'a N,
    contract_store: &'a ContractStore,
    key_store: &'a KeyStore,
    note_store: &'a NoteStore,
    capsule_store: &'a CapsuleStore,
    /// The block header at which execution is anchored.
    #[allow(dead_code)] // Used when ACVM integration is complete
    block_header: serde_json::Value,
    /// Execution-scoped note cache for transient notes.
    note_cache: Vec<CachedNote>,
    /// Nullifiers emitted during this execution.
    nullifier_cache: Vec<Fr>,
    /// The address of the contract being executed.
    contract_address: AztecAddress,
}

/// A note created during execution (not yet committed to state).
#[derive(Debug, Clone)]
pub struct CachedNote {
    pub contract_address: AztecAddress,
    pub storage_slot: Fr,
    pub note_hash: Fr,
    pub note_data: Vec<Fr>,
}

impl<'a, N: AztecNode> PrivateExecutionOracle<'a, N> {
    pub fn new(
        node: &'a N,
        contract_store: &'a ContractStore,
        key_store: &'a KeyStore,
        note_store: &'a NoteStore,
        capsule_store: &'a CapsuleStore,
        block_header: serde_json::Value,
        contract_address: AztecAddress,
    ) -> Self {
        Self {
            node,
            contract_store,
            key_store,
            note_store,
            capsule_store,
            block_header,
            note_cache: Vec::new(),
            nullifier_cache: Vec::new(),
            contract_address,
        }
    }

    /// Handle an ACVM foreign call by name and arguments.
    ///
    /// Returns the result as a list of field element vectors (matching the ACVM
    /// foreign call response format).
    pub async fn handle_foreign_call(
        &mut self,
        name: &str,
        args: &[Vec<Fr>],
    ) -> Result<Vec<Vec<Fr>>, Error> {
        match name {
            "getSecretKey" => self.get_secret_key(args).await,
            "getPublicKeysAndPartialAddress" => {
                self.get_public_keys_and_partial_address(args).await
            }
            "getNotes" => self.get_notes(args).await,
            "checkNoteHashExists" => self.check_note_hash_exists(args).await,
            "getPublicStorageAt" => self.get_public_storage_at(args).await,
            "getContractInstance" => self.get_contract_instance(args).await,
            "getCapsule" => self.get_capsule(args).await,
            "getBlockHeader" => self.get_block_header(args).await,
            "emitNote" => self.emit_note(args),
            "emitNullifier" => self.emit_nullifier(args),
            _ => Err(Error::InvalidData(format!("unknown oracle call: {name}"))),
        }
    }

    async fn get_secret_key(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let pk_hash = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("getSecretKey: missing pk_hash".into()))?;
        let sk = self
            .key_store
            .get_secret_key(pk_hash)
            .await?
            .ok_or_else(|| Error::InvalidData("account not found in key store".into()))?;
        Ok(vec![vec![sk]])
    }

    async fn get_public_keys_and_partial_address(
        &self,
        args: &[Vec<Fr>],
    ) -> Result<Vec<Vec<Fr>>, Error> {
        let pk_hash = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing pk_hash arg".into()))?;
        let public_keys = self
            .key_store
            .get_public_keys(pk_hash)
            .await?
            .ok_or_else(|| Error::InvalidData("account not found".into()))?;
        // Return public keys as flat field elements
        Ok(vec![vec![
            public_keys.master_nullifier_public_key.x,
            public_keys.master_nullifier_public_key.y,
            public_keys.master_incoming_viewing_public_key.x,
            public_keys.master_incoming_viewing_public_key.y,
            public_keys.master_outgoing_viewing_public_key.x,
            public_keys.master_outgoing_viewing_public_key.y,
            public_keys.master_tagging_public_key.x,
            public_keys.master_tagging_public_key.y,
        ]])
    }

    async fn get_notes(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let storage_slot = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("getNotes: missing storage_slot".into()))?;
        let notes = self
            .note_store
            .get_notes_by_slot(&self.contract_address, storage_slot)
            .await?;
        // Return notes as flattened field data
        let result: Vec<Fr> = notes.into_iter().flat_map(|n| n.note_data).collect();
        Ok(vec![result])
    }

    async fn check_note_hash_exists(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let note_hash = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing note_hash".into()))?;
        let exists = self
            .note_store
            .has_note(&self.contract_address, note_hash)
            .await?;
        Ok(vec![vec![Fr::from(exists)]])
    }

    async fn get_public_storage_at(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let contract = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing contract address".into()))?;
        let slot = args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing storage slot".into()))?;
        let contract_addr = AztecAddress(*contract);
        let value = self
            .node
            .get_public_storage_at(0, &contract_addr, slot)
            .await?;
        Ok(vec![vec![value]])
    }

    async fn get_contract_instance(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let address = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing address".into()))?;
        let addr = AztecAddress(*address);

        // Check local store first, then node
        if let Some(inst) = self.contract_store.get_instance(&addr).await? {
            return Ok(vec![vec![
                Fr::from(true), // exists
                inst.inner.salt,
                Fr::from(inst.inner.deployer),
                inst.inner.current_contract_class_id,
                inst.inner.initialization_hash,
            ]]);
        }

        if let Some(inst) = self.node.get_contract(&addr).await? {
            return Ok(vec![vec![
                Fr::from(true),
                inst.inner.salt,
                Fr::from(inst.inner.deployer),
                inst.inner.current_contract_class_id,
                inst.inner.initialization_hash,
            ]]);
        }

        Ok(vec![vec![Fr::from(false)]])
    }

    async fn get_capsule(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let contract_id = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing capsule contract id".into()))?;
        match self.capsule_store.pop(contract_id).await? {
            Some(capsule) => Ok(capsule),
            None => Err(Error::InvalidData("no capsule available".into())),
        }
    }

    async fn get_block_header(&self, _args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        // The block header is opaque JSON; for the oracle we return an empty
        // response and rely on the executor setting it in the initial witness.
        Ok(vec![vec![]])
    }

    fn emit_note(&mut self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let storage_slot = args
            .first()
            .and_then(|v| v.first())
            .copied()
            .ok_or_else(|| Error::InvalidData("emitNote: missing storage_slot".into()))?;
        let note_hash = args
            .get(1)
            .and_then(|v| v.first())
            .copied()
            .ok_or_else(|| Error::InvalidData("emitNote: missing note_hash".into()))?;
        let note_data = args.get(2).cloned().unwrap_or_default();
        self.note_cache.push(CachedNote {
            contract_address: self.contract_address,
            storage_slot,
            note_hash,
            note_data,
        });
        Ok(vec![vec![]])
    }

    fn emit_nullifier(&mut self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let nullifier = args
            .first()
            .and_then(|v| v.first())
            .copied()
            .ok_or_else(|| Error::InvalidData("emitNullifier: missing nullifier".into()))?;
        self.nullifier_cache.push(nullifier);
        Ok(vec![vec![]])
    }

    /// Get the notes created during this execution.
    pub fn cached_notes(&self) -> &[CachedNote] {
        &self.note_cache
    }

    /// Get the nullifiers emitted during this execution.
    pub fn cached_nullifiers(&self) -> &[Fr] {
        &self.nullifier_cache
    }
}
