//! Private kernel oracle for providing hints during kernel circuit execution.
//!
//! Ports the TS `PrivateKernelOracle` which provides contract preimages,
//! membership witnesses, VK witnesses, and other data needed by kernel circuits.

use aztec_core::abi::{ContractArtifact, FunctionSelector};
use aztec_core::error::Error;
use aztec_core::hash::{
    compute_artifact_hash, compute_private_functions_root_from_artifact,
    compute_public_bytecode_commitment, compute_salted_initialization_hash,
};
use aztec_core::types::{AztecAddress, Fr};
use aztec_node_client::AztecNode;

use crate::stores::{ContractStore, KeyStore};

/// Oracle for private kernel circuit interactions with state trees.
///
/// Provides hints and data lookups needed during kernel circuit execution:
/// - Contract address and class preimages
/// - Function membership witnesses
/// - VK membership witnesses in the protocol VK tree
/// - Note hash and nullifier tree membership witnesses
/// - Master secret keys for key verification
pub struct PrivateKernelOracle<'a, N: AztecNode> {
    node: &'a N,
    contract_store: &'a ContractStore,
    key_store: &'a KeyStore,
    /// Block hash for consistent state reads.
    block_hash: Fr,
}

impl<'a, N: AztecNode> PrivateKernelOracle<'a, N> {
    pub fn new(
        node: &'a N,
        contract_store: &'a ContractStore,
        key_store: &'a KeyStore,
        block_hash: Fr,
    ) -> Self {
        Self {
            node,
            contract_store,
            key_store,
            block_hash,
        }
    }

    /// Get the contract address preimage (instance data including salted init hash).
    ///
    /// Returns the contract instance data needed to verify the contract address
    /// derivation inside the kernel circuit.
    pub async fn get_contract_address_preimage(
        &self,
        address: &AztecAddress,
    ) -> Result<serde_json::Value, Error> {
        let instance = if let Some(instance) = self.contract_store.get_instance(address).await? {
            instance
        } else {
            match self.node.get_contract(address).await? {
                Some(instance) => instance,
                None => {
                    return Err(Error::InvalidData(format!(
                        "contract instance not found for address {address}"
                    )))
                }
            }
        };

        let salted_initialization_hash = compute_salted_initialization_hash(
            instance.inner.salt,
            instance.inner.initialization_hash,
            instance.inner.deployer,
        );

        Ok(serde_json::json!({
            "address": instance.address,
            "saltedInitializationHash": salted_initialization_hash,
            "version": instance.inner.version,
            "salt": instance.inner.salt,
            "deployer": instance.inner.deployer,
            "currentContractClassId": instance.inner.current_contract_class_id,
            "originalContractClassId": instance.inner.original_contract_class_id,
            "initializationHash": instance.inner.initialization_hash,
            "publicKeys": instance.inner.public_keys,
        }))
    }

    fn contract_class_preimage(artifact: &ContractArtifact) -> Result<serde_json::Value, Error> {
        let artifact_hash = compute_artifact_hash(artifact);
        let private_functions_root = compute_private_functions_root_from_artifact(artifact)?;
        let public_bytecode_commitment =
            compute_public_bytecode_commitment(&extract_packed_public_bytecode(artifact));

        Ok(serde_json::json!({
            "artifactHash": artifact_hash,
            "privateFunctionsRoot": private_functions_root,
            "publicBytecodeCommitment": public_bytecode_commitment,
        }))
    }

    /// Get the contract class ID preimage (artifact hash, bytecode commitment).
    ///
    /// Returns the contract class data needed to verify class ID derivation.
    pub async fn get_contract_class_id_preimage(
        &self,
        class_id: &Fr,
    ) -> Result<serde_json::Value, Error> {
        if let Some(artifact) = self.contract_store.get_artifact(class_id).await? {
            return Self::contract_class_preimage(&artifact);
        }

        match self.node.get_contract_class(class_id).await? {
            Some(class_data) => Ok(class_data),
            None => Err(Error::InvalidData(format!(
                "contract class not found for id {class_id}"
            ))),
        }
    }

    /// Get function membership witness in the contract's private function tree.
    ///
    /// Proves that a function selector belongs to a specific contract class.
    pub async fn get_function_membership_witness(
        &self,
        class_id: &Fr,
        function_selector: &Fr,
    ) -> Result<serde_json::Value, Error> {
        Err(Error::InvalidData(format!(
            "private function membership witness for class {class_id} selector {function_selector} is not implemented yet"
        )))
    }

    /// Get VK membership witness in the protocol VK indexed merkle tree.
    ///
    /// Proves that a verification key is part of the protocol's VK tree.
    pub async fn get_vk_membership_witness(
        &self,
        vk_hash: &Fr,
    ) -> Result<serde_json::Value, Error> {
        Err(Error::InvalidData(format!(
            "protocol VK membership witness for vk hash {vk_hash} is not implemented yet"
        )))
    }

    /// Get note hash membership witness at the current block.
    pub async fn get_note_hash_membership_witness(
        &self,
        note_hash: &Fr,
    ) -> Result<Option<serde_json::Value>, Error> {
        // Use block number 0 to indicate "at the anchor block"
        self.node
            .get_note_hash_membership_witness(0, note_hash)
            .await
    }

    /// Get nullifier membership witness at the current block.
    pub async fn get_nullifier_membership_witness(
        &self,
        nullifier: &Fr,
    ) -> Result<Option<serde_json::Value>, Error> {
        self.node
            .get_nullifier_membership_witness(0, nullifier)
            .await
    }

    /// Get the note hash tree root from the block header.
    pub async fn get_note_hash_tree_root(&self) -> Result<Fr, Error> {
        let header = self.node.get_block_header(0).await?;
        // Extract note hash tree root from header JSON
        if let Some(root) = header
            .pointer("/state/partial/noteHashTree/root")
            .and_then(|v| v.as_str())
        {
            Fr::from_hex(root)
        } else {
            Err(Error::InvalidData(
                "note hash tree root not found in block header".into(),
            ))
        }
    }

    /// Get the master secret key (sk_m) for key verification in the kernel.
    pub async fn get_master_secret_key(&self, pk_hash: &Fr) -> Result<Option<Fr>, Error> {
        self.key_store.get_secret_key(pk_hash).await
    }

    /// Get block hash membership witness in the archive tree.
    pub async fn get_block_hash_membership_witness(
        &self,
        block_hash: &Fr,
    ) -> Result<Option<serde_json::Value>, Error> {
        self.node
            .get_block_hash_membership_witness(0, block_hash)
            .await
    }

    /// Get updated class ID hints (public data witnesses for class ID updates).
    pub async fn get_updated_class_id_hints(
        &self,
        address: &AztecAddress,
    ) -> Result<serde_json::Value, Error> {
        Err(Error::InvalidData(format!(
            "updated class-id hints for contract {address} are not implemented yet"
        )))
    }

    /// Get debug function name from selector (for error messages).
    pub async fn get_debug_function_name(
        &self,
        contract_address: &AztecAddress,
        function_selector: &FunctionSelector,
    ) -> Result<Option<String>, Error> {
        if let Some(instance) = self.contract_store.get_instance(contract_address).await? {
            if let Some(artifact) = self
                .contract_store
                .get_artifact(&instance.inner.current_contract_class_id)
                .await?
            {
                for func in &artifact.functions {
                    if let Some(ref sel) = func.selector {
                        if sel == function_selector {
                            return Ok(Some(func.name.clone()));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// Get the block hash used for consistent state reads.
    pub fn block_hash(&self) -> &Fr {
        &self.block_hash
    }
}

fn extract_packed_public_bytecode(artifact: &ContractArtifact) -> Vec<u8> {
    // Only public_dispatch carries the packed bytecode (mirrors TS retainBytecode filter).
    artifact
        .functions
        .iter()
        .find(|f| {
            f.function_type == aztec_core::abi::FunctionType::Public && f.name == "public_dispatch"
        })
        .and_then(|f| f.bytecode.as_deref())
        .map(|bc| decode_bytecode(bc))
        .unwrap_or_default()
}

fn decode_bytecode(encoded: &str) -> Vec<u8> {
    let Some(hex) = encoded.strip_prefix("0x") else {
        return Vec::new();
    };
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let mut chars = hex.as_bytes().chunks_exact(2);
    for pair in &mut chars {
        if let Ok(pair_str) = std::str::from_utf8(pair) {
            if let Ok(byte) = u8::from_str_radix(pair_str, 16) {
                bytes.push(byte);
            } else {
                return Vec::new();
            }
        } else {
            return Vec::new();
        }
    }
    if !chars.remainder().is_empty() {
        return Vec::new();
    }
    bytes
}
