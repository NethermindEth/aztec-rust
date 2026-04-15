//! Private kernel oracle for providing hints during kernel circuit execution.
//!
//! Ports the TS `PrivateKernelOracle` which provides contract preimages,
//! membership witnesses, VK witnesses, and other data needed by kernel circuits.

use aztec_core::abi::{ContractArtifact, FunctionSelector, FunctionType};
use aztec_core::constants::{self, domain_separator};
use aztec_core::error::Error;
use aztec_core::hash::{
    compute_artifact_hash, compute_private_functions_root_from_artifact,
    compute_public_bytecode_commitment, compute_salted_initialization_hash, poseidon2_hash,
    poseidon2_hash_with_separator,
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
        if let Some(preimage) = self.contract_store.get_class_preimage(class_id).await? {
            return Ok(preimage);
        }

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
        let artifact = self
            .contract_store
            .get_artifact(class_id)
            .await?
            .ok_or_else(|| {
                Error::InvalidData(format!(
                    "contract artifact not found for private function witness class {class_id}"
                ))
            })?;
        let selector = FunctionSelector::from_field(*function_selector);
        let (leaf_index, sibling_path) = private_function_membership_witness(&artifact, selector)?;
        Ok(membership_witness_json(leaf_index, sibling_path))
    }

    /// Get VK membership witness in the protocol VK indexed merkle tree.
    ///
    /// Proves that a verification key is part of the protocol's VK tree.
    pub async fn get_vk_membership_witness(
        &self,
        vk_hash: &Fr,
    ) -> Result<serde_json::Value, Error> {
        let tree = load_vk_tree()?;
        let leaf_count = (tree.nodes.len() + 1) / 2;
        let leaf_index = tree
            .nodes
            .iter()
            .take(leaf_count)
            .position(|leaf| leaf == vk_hash)
            .ok_or_else(|| Error::InvalidData(format!("VK hash {vk_hash} not found in VK tree")))?;
        let sibling_path = sibling_path_from_flat_tree(&tree.nodes, leaf_index)?;
        Ok(membership_witness_json(leaf_index, sibling_path))
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

fn membership_witness_json(leaf_index: usize, sibling_path: Vec<Fr>) -> serde_json::Value {
    serde_json::json!({
        "leafIndex": leaf_index,
        "siblingPath": sibling_path,
    })
}

fn private_function_membership_witness(
    artifact: &ContractArtifact,
    selector: FunctionSelector,
) -> Result<(usize, Vec<Fr>), Error> {
    let mut private_fns: Vec<(FunctionSelector, Fr)> = artifact
        .functions
        .iter()
        .filter(|function| function.function_type == FunctionType::Private)
        .map(|function| {
            let selector = function.selector.unwrap_or_else(|| {
                FunctionSelector::from_name_and_parameters(&function.name, &function.parameters)
            });
            let vk_hash = function.verification_key_hash.unwrap_or(Fr::zero());
            (selector, vk_hash)
        })
        .collect();
    private_fns.sort_by_key(|(selector, _)| u32::from_be_bytes(selector.0));

    let leaf_index = private_fns
        .iter()
        .position(|(function_selector, _)| *function_selector == selector)
        .ok_or_else(|| {
            Error::InvalidData(format!(
                "private function selector {selector} not found in artifact {}",
                artifact.name
            ))
        })?;

    let leaf_count = 1usize << constants::FUNCTION_TREE_HEIGHT;
    let zero_leaf = poseidon2_hash(&[Fr::zero(), Fr::zero()]);
    let mut leaves = private_fns
        .into_iter()
        .map(|(selector, vk_hash)| {
            poseidon2_hash_with_separator(
                &[selector.to_field(), vk_hash],
                domain_separator::PRIVATE_FUNCTION_LEAF,
            )
        })
        .collect::<Vec<_>>();
    leaves.resize(leaf_count, zero_leaf);

    Ok((leaf_index, sibling_path_from_leaves(leaves, leaf_index)?))
}

fn sibling_path_from_leaves(mut level: Vec<Fr>, leaf_index: usize) -> Result<Vec<Fr>, Error> {
    if level.is_empty() || leaf_index >= level.len() {
        return Err(Error::InvalidData(format!(
            "invalid leaf index {leaf_index} for tree with {} leaves",
            level.len()
        )));
    }

    let mut index = leaf_index;
    let mut sibling_path = Vec::new();
    while level.len() > 1 {
        let sibling_index = if index & 1 == 1 { index - 1 } else { index + 1 };
        sibling_path.push(level.get(sibling_index).copied().unwrap_or_else(Fr::zero));

        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        for chunk in level.chunks(2) {
            let left = chunk[0];
            let right = *chunk.get(1).unwrap_or(&Fr::zero());
            next.push(poseidon2_hash(&[left, right]));
        }
        level = next;
        index >>= 1;
    }
    Ok(sibling_path)
}

#[derive(Debug)]
struct VkTree {
    nodes: Vec<Fr>,
}

fn load_vk_tree() -> Result<VkTree, Error> {
    let path = locate_vk_tree_path().ok_or_else(|| {
        Error::InvalidData(
            "VK tree not found; set PXE_VK_TREE_PATH or AZTEC_PACKAGES_PATH so Rust can load noir-protocol-circuits-types/src/vk_tree.ts".into(),
        )
    })?;
    let contents = std::fs::read_to_string(&path).map_err(|err| {
        Error::InvalidData(format!(
            "failed to read VK tree from {}: {err}",
            path.display()
        ))
    })?;
    let nodes = parse_vk_tree_nodes(&contents)?;
    Ok(VkTree { nodes })
}

fn locate_vk_tree_path() -> Option<std::path::PathBuf> {
    if let Ok(path) = std::env::var("PXE_VK_TREE_PATH") {
        let path = std::path::PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }

    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("AZTEC_PACKAGES_PATH") {
        let root = std::path::PathBuf::from(path);
        candidates.push(root.join("yarn-project/noir-protocol-circuits-types/src/vk_tree.ts"));
        candidates.push(root.join("noir-protocol-circuits-types/src/vk_tree.ts"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(
            cwd.join("../aztec-packages/yarn-project/noir-protocol-circuits-types/src/vk_tree.ts"),
        );
        candidates.push(
            cwd.join(
                "../../aztec-packages/yarn-project/noir-protocol-circuits-types/src/vk_tree.ts",
            ),
        );
    }

    candidates.into_iter().find(|path| path.exists())
}

fn parse_vk_tree_nodes(contents: &str) -> Result<Vec<Fr>, Error> {
    let mut nodes = Vec::new();
    let bytes = contents.as_bytes();
    let mut i = 0;
    while i + 66 <= bytes.len() {
        if bytes[i] == b'\'' {
            let hex_start = i + 1;
            let hex_end = hex_start + 64;
            if hex_end < bytes.len()
                && bytes[hex_end] == b'\''
                && bytes[hex_start..hex_end]
                    .iter()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                let hex = std::str::from_utf8(&bytes[hex_start..hex_end]).map_err(|err| {
                    Error::InvalidData(format!("invalid UTF-8 in VK tree hex node: {err}"))
                })?;
                nodes.push(Fr::from_hex(&format!("0x{hex}"))?);
                i = hex_end + 1;
                continue;
            }
        }
        i += 1;
    }

    if nodes.is_empty() {
        return Err(Error::InvalidData("VK tree contains no nodes".into()));
    }
    if !nodes.len().is_power_of_two() && (nodes.len() + 1).is_power_of_two() {
        Ok(nodes)
    } else {
        Err(Error::InvalidData(format!(
            "VK tree has invalid flat node count {}; expected 2^(height + 1) - 1",
            nodes.len()
        )))
    }
}

fn sibling_path_from_flat_tree(nodes: &[Fr], leaf_index: usize) -> Result<Vec<Fr>, Error> {
    let leaf_count = (nodes.len() + 1) / 2;
    if leaf_index >= leaf_count {
        return Err(Error::InvalidData(format!(
            "invalid VK leaf index {leaf_index} for {leaf_count} leaves"
        )));
    }

    let mut row_size = leaf_count;
    let mut row_offset = 0usize;
    let mut index = leaf_index;
    let mut sibling_path = Vec::new();
    while row_size > 1 {
        let sibling_index = if index & 1 == 1 { index - 1 } else { index + 1 };
        sibling_path.push(nodes[row_offset + sibling_index]);
        row_offset += row_size;
        row_size >>= 1;
        index >>= 1;
    }
    Ok(sibling_path)
}

fn decode_bytecode(encoded: &str) -> Vec<u8> {
    if let Some(hex) = encoded.strip_prefix("0x") {
        let mut bytes = Vec::with_capacity(hex.len() / 2);
        let mut chunks = hex.as_bytes().chunks_exact(2);
        for pair in &mut chunks {
            let Ok(pair) = std::str::from_utf8(pair) else {
                return Vec::new();
            };
            let Ok(byte) = u8::from_str_radix(pair, 16) else {
                return Vec::new();
            };
            bytes.push(byte);
        }
        if !chunks.remainder().is_empty() {
            return Vec::new();
        }
        return bytes;
    }

    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap_or_default()
}
