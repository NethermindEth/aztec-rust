//! Oracle for utility (view/unconstrained) function execution.

use aztec_core::error::Error;
use aztec_core::types::{AztecAddress, Fr};
use aztec_node_client::AztecNode;

use crate::stores::{ContractStore, KeyStore, NoteStore};

/// Oracle for utility function execution (read-only, no side effects).
pub struct UtilityExecutionOracle<'a, N: AztecNode> {
    node: &'a N,
    contract_store: &'a ContractStore,
    key_store: &'a KeyStore,
    note_store: &'a NoteStore,
    contract_address: AztecAddress,
}

impl<'a, N: AztecNode> UtilityExecutionOracle<'a, N> {
    pub fn new(
        node: &'a N,
        contract_store: &'a ContractStore,
        key_store: &'a KeyStore,
        note_store: &'a NoteStore,
        contract_address: AztecAddress,
    ) -> Self {
        Self {
            node,
            contract_store,
            key_store,
            note_store,
            contract_address,
        }
    }

    /// Handle an ACVM foreign call for a utility function.
    pub async fn handle_foreign_call(
        &self,
        name: &str,
        args: &[Vec<Fr>],
    ) -> Result<Vec<Vec<Fr>>, Error> {
        match name {
            "getPublicStorageAt" => self.get_public_storage_at(args).await,
            "getContractInstance" => self.get_contract_instance(args).await,
            "getNotes" => self.get_notes(args).await,
            "getPublicKeysAndPartialAddress" => {
                self.get_public_keys_and_partial_address(args).await
            }
            _ => Err(Error::InvalidData(format!(
                "unknown utility oracle call: {name}"
            ))),
        }
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

        if let Some(inst) = self.contract_store.get_instance(&addr).await? {
            return Ok(vec![vec![
                Fr::from(true),
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

    async fn get_notes(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let storage_slot = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("getNotes: missing storage_slot".into()))?;
        let notes = self
            .note_store
            .get_notes_by_slot(&self.contract_address, storage_slot)
            .await?;
        let result: Vec<Fr> = notes.into_iter().flat_map(|n| n.note_data).collect();
        Ok(vec![result])
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
}
