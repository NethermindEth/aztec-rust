//! Poseidon2 hash functions for the Aztec protocol.
//!
//! Provides `poseidon2_hash_with_separator` and derived functions that mirror the
//! TypeScript SDK's hashing utilities.

use sha2::{Digest, Sha256};

use crate::abi::{
    encode_arguments, AbiValue, ContractArtifact, FunctionArtifact, FunctionSelector, FunctionType,
};
use crate::constants::{self, domain_separator};
use crate::grumpkin;
use crate::tx::FunctionCall;
use crate::types::{AztecAddress, ContractInstance, Fq, Fr, PublicKeys};
use crate::Error;

/// Compute a Poseidon2 sponge hash over `inputs`.
///
/// Uses a rate-3 / capacity-1 sponge with the standard Aztec IV construction:
/// `state[3] = len * 2^64`.
///
/// This matches barretenberg's `poseidon2_hash` and the TS SDK's `poseidon2Hash`.
pub(crate) fn poseidon2_hash(inputs: &[Fr]) -> Fr {
    use ark_bn254::Fr as ArkFr;
    use taceo_poseidon2::bn254::t4::permutation;

    const RATE: usize = 3;

    // IV: capacity element = input_length * 2^64
    let two_pow_64 = ArkFr::from(1u64 << 32) * ArkFr::from(1u64 << 32);
    let iv = ArkFr::from(inputs.len() as u64) * two_pow_64;

    let mut state: [ArkFr; 4] = [ArkFr::from(0u64), ArkFr::from(0u64), ArkFr::from(0u64), iv];
    let mut cache = [ArkFr::from(0u64); RATE];
    let mut cache_size = 0usize;

    for input in inputs {
        if cache_size == RATE {
            for i in 0..RATE {
                state[i] += cache[i];
            }
            cache = [ArkFr::from(0u64); RATE];
            cache_size = 0;
            state = permutation(&state);
        }
        cache[cache_size] = input.0;
        cache_size += 1;
    }

    // Absorb remaining cache
    for i in 0..cache_size {
        state[i] += cache[i];
    }
    state = permutation(&state);

    Fr(state[0])
}

/// Compute a Poseidon2 hash over raw bytes using the same chunking as Aztec's
/// `poseidon2HashBytes`.
///
/// Bytes are split into 31-byte chunks, each chunk is placed into a 32-byte
/// buffer, reversed, and then interpreted as a field element before hashing.
pub(crate) fn poseidon2_hash_bytes(bytes: &[u8]) -> Fr {
    if bytes.is_empty() {
        return poseidon2_hash(&[]);
    }

    let inputs = bytes
        .chunks(31)
        .map(|chunk| {
            let mut field_bytes = [0u8; 32];
            field_bytes[..chunk.len()].copy_from_slice(chunk);
            field_bytes.reverse();
            Fr::from(field_bytes)
        })
        .collect::<Vec<_>>();

    poseidon2_hash(&inputs)
}

/// Compute a Poseidon2 hash of `inputs` with a domain separator prepended.
///
/// Mirrors the TS `poseidon2HashWithSeparator(args, separator)`.
pub fn poseidon2_hash_with_separator(inputs: &[Fr], separator: u32) -> Fr {
    poseidon2_hash_with_separator_field(inputs, Fr::from(u64::from(separator)))
}

/// Compute a Poseidon2 hash of `inputs` with a full field-element domain separator prepended.
pub fn poseidon2_hash_with_separator_field(inputs: &[Fr], separator: Fr) -> Fr {
    let mut full_input = Vec::with_capacity(1 + inputs.len());
    full_input.push(separator);
    full_input.extend_from_slice(inputs);
    poseidon2_hash(&full_input)
}

/// Hash a secret for use in L1-L2 message flow and TransparentNote.
///
/// `secret_hash = poseidon2([secret], SECRET_HASH)`
///
/// Mirrors TS `computeSecretHash(secret)`.
pub fn compute_secret_hash(secret: &Fr) -> Fr {
    poseidon2_hash_with_separator(&[*secret], domain_separator::SECRET_HASH)
}

/// Hash function arguments using Poseidon2 with the `FUNCTION_ARGS` separator.
///
/// Returns `Fr::zero()` if `args` is empty.
///
/// Mirrors TS `computeVarArgsHash(args)`.
pub fn compute_var_args_hash(args: &[Fr]) -> Fr {
    if args.is_empty() {
        return Fr::zero();
    }
    poseidon2_hash_with_separator(args, domain_separator::FUNCTION_ARGS)
}

/// Compute the inner authwit hash — the "intent" before siloing with consumer.
///
/// `args` is typically `[caller, selector, args_hash]`.
/// Uses Poseidon2 with `AUTHWIT_INNER` domain separator.
///
/// Mirrors TS `computeInnerAuthWitHash(args)`.
pub fn compute_inner_auth_wit_hash(args: &[Fr]) -> Fr {
    poseidon2_hash_with_separator(args, domain_separator::AUTHWIT_INNER)
}

/// Compute the outer authwit hash — the value the approver signs.
///
/// Combines consumer address, chain ID, protocol version, and inner hash.
/// Uses Poseidon2 with `AUTHWIT_OUTER` domain separator.
///
/// Mirrors TS `computeOuterAuthWitHash(consumer, chainId, version, innerHash)`.
pub fn compute_outer_auth_wit_hash(
    consumer: &AztecAddress,
    chain_id: &Fr,
    version: &Fr,
    inner_hash: &Fr,
) -> Fr {
    poseidon2_hash_with_separator(
        &[consumer.0, *chain_id, *version, *inner_hash],
        domain_separator::AUTHWIT_OUTER,
    )
}

/// Flatten ABI values into their field element representation.
///
/// Handles the common types used in authwit scenarios: `Field`, `Boolean`,
/// `Integer`, `Array`, `Struct`, and `Tuple`. Strings are encoded as
/// one field element per byte.
pub fn abi_values_to_fields(args: &[AbiValue]) -> Vec<Fr> {
    let mut fields = Vec::new();
    for arg in args {
        flatten_abi_value(arg, &mut fields);
    }
    fields
}

fn flatten_abi_value(value: &AbiValue, out: &mut Vec<Fr>) {
    match value {
        AbiValue::Field(f) => out.push(*f),
        AbiValue::Boolean(b) => out.push(if *b { Fr::one() } else { Fr::zero() }),
        AbiValue::Integer(i) => {
            // Integers are encoded as unsigned field elements.
            // Negative values are not expected in authwit args.
            out.push(Fr::from(*i as u64));
        }
        AbiValue::Array(items) => {
            for item in items {
                flatten_abi_value(item, out);
            }
        }
        AbiValue::String(s) => {
            for byte in s.bytes() {
                out.push(Fr::from(u64::from(byte)));
            }
        }
        AbiValue::Struct(map) => {
            // BTreeMap iterates in key order (deterministic).
            for value in map.values() {
                flatten_abi_value(value, out);
            }
        }
        AbiValue::Tuple(items) => {
            for item in items {
                flatten_abi_value(item, out);
            }
        }
    }
}

/// Compute the inner authwit hash from a caller address and a function call.
///
/// Computes `computeInnerAuthWitHash([caller, call.selector, varArgsHash(call.args)])`.
///
/// Mirrors TS `computeInnerAuthWitHashFromAction(caller, action)`.
pub fn compute_inner_auth_wit_hash_from_action(caller: &AztecAddress, call: &FunctionCall) -> Fr {
    let args_as_fields = abi_values_to_fields(&call.args);
    let args_hash = compute_var_args_hash(&args_as_fields);
    compute_inner_auth_wit_hash(&[caller.0, call.selector.to_field(), args_hash])
}

/// Chain identification information.
///
/// This is defined in `aztec-core` so that hash functions can use it
/// without creating a circular dependency with `aztec-wallet`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainInfo {
    /// The L2 chain ID.
    pub chain_id: Fr,
    /// The rollup protocol version.
    pub version: Fr,
}

/// Either a raw message hash, a structured call intent, or a pre-computed
/// inner hash with its consumer address.
///
/// Mirrors the TS distinction between `Fr`, `CallIntent`, and `IntentInnerHash`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageHashOrIntent {
    /// A raw message hash (already computed outer hash).
    Hash {
        /// The hash value.
        hash: Fr,
    },
    /// A structured call intent.
    Intent {
        /// The caller requesting authorization.
        caller: AztecAddress,
        /// The function call to authorize.
        call: FunctionCall,
    },
    /// A pre-computed inner hash with consumer address.
    ///
    /// Used when the inner hash is already known but the outer hash
    /// (which includes chain info) still needs to be computed.
    InnerHash {
        /// The consumer contract address.
        consumer: AztecAddress,
        /// The inner hash value.
        inner_hash: Fr,
    },
}

/// Compute the full authwit message hash from an intent and chain info.
///
/// For `MessageHashOrIntent::Hash` — returns the hash directly.
/// For `MessageHashOrIntent::Intent { caller, call }`:
///   1. `inner_hash = compute_inner_auth_wit_hash_from_action(caller, call)`
///   2. `consumer = call.to` (the contract being called)
///   3. `outer_hash = compute_outer_auth_wit_hash(consumer, chain_id, version, inner_hash)`
///
/// For `MessageHashOrIntent::InnerHash { consumer, inner_hash }`:
///   1. `outer_hash = compute_outer_auth_wit_hash(consumer, chain_id, version, inner_hash)`
///
/// Mirrors TS `computeAuthWitMessageHash(intent, metadata)`.
pub fn compute_auth_wit_message_hash(intent: &MessageHashOrIntent, chain_info: &ChainInfo) -> Fr {
    match intent {
        MessageHashOrIntent::Hash { hash } => *hash,
        MessageHashOrIntent::Intent { caller, call } => {
            let inner_hash = compute_inner_auth_wit_hash_from_action(caller, call);
            compute_outer_auth_wit_hash(
                &call.to,
                &chain_info.chain_id,
                &chain_info.version,
                &inner_hash,
            )
        }
        MessageHashOrIntent::InnerHash {
            consumer,
            inner_hash,
        } => compute_outer_auth_wit_hash(
            consumer,
            &chain_info.chain_id,
            &chain_info.version,
            inner_hash,
        ),
    }
}

// ---------------------------------------------------------------------------
// Deployment hash primitives
// ---------------------------------------------------------------------------

/// Compute the initialization hash for a contract deployment.
///
/// Returns `Fr::zero()` if `init_fn` is `None` (no constructor).
///
/// Formula: `poseidon2_hash_with_separator([selector, args_hash], INITIALIZER)`
pub fn compute_initialization_hash(
    init_fn: Option<&FunctionArtifact>,
    args: &[AbiValue],
) -> Result<Fr, Error> {
    match init_fn {
        None => Ok(Fr::zero()),
        Some(func) => {
            let selector = func.selector.unwrap_or_else(|| {
                FunctionSelector::from_name_and_parameters(&func.name, &func.parameters)
            });
            let encoded_args = encode_arguments(func, args)?;
            let args_hash = compute_var_args_hash(&encoded_args);
            Ok(poseidon2_hash_with_separator(
                &[selector.to_field(), args_hash],
                domain_separator::INITIALIZER,
            ))
        }
    }
}

/// Compute initialization hash from pre-encoded selector and args.
pub fn compute_initialization_hash_from_encoded(selector: Fr, encoded_args: &[Fr]) -> Fr {
    let args_hash = compute_var_args_hash(encoded_args);
    poseidon2_hash_with_separator(&[selector, args_hash], domain_separator::INITIALIZER)
}

// ---------------------------------------------------------------------------
// Contract class ID computation (Step 5.5)
// ---------------------------------------------------------------------------

/// Compute the root of the private functions Merkle tree.
///
/// Each leaf = `poseidon2_hash_with_separator([selector, vk_hash], PRIVATE_FUNCTION_LEAF)`.
/// Tree height = `FUNCTION_TREE_HEIGHT` (7).
pub fn compute_private_functions_root(private_functions: &mut [(FunctionSelector, Fr)]) -> Fr {
    let tree_height = constants::FUNCTION_TREE_HEIGHT;
    let num_leaves = 1usize << tree_height; // 128

    // Sort by selector bytes (big-endian u32 value).
    private_functions.sort_by_key(|(sel, _)| u32::from_be_bytes(sel.0));

    // Compute leaves.
    let zero_leaf = poseidon2_hash(&[Fr::zero(), Fr::zero()]);
    let mut leaves: Vec<Fr> = Vec::with_capacity(num_leaves);
    for (sel, vk_hash) in private_functions.iter() {
        let leaf = poseidon2_hash_with_separator(
            &[sel.to_field(), *vk_hash],
            domain_separator::PRIVATE_FUNCTION_LEAF,
        );
        leaves.push(leaf);
    }
    // Pad remaining leaves with zeros.
    leaves.resize(num_leaves, zero_leaf);

    // Build Merkle tree bottom-up using Poseidon2 for internal nodes.
    poseidon_merkle_root(&leaves)
}

/// Build a binary Merkle tree root from leaves using Poseidon2.
fn poseidon_merkle_root(leaves: &[Fr]) -> Fr {
    if leaves.is_empty() {
        return Fr::zero();
    }
    if leaves.len() == 1 {
        return leaves[0];
    }

    let mut current = leaves.to_vec();
    while current.len() > 1 {
        let mut next = Vec::with_capacity(current.len().div_ceil(2));
        for chunk in current.chunks(2) {
            let left = chunk[0];
            let right = if chunk.len() > 1 {
                chunk[1]
            } else {
                Fr::zero()
            };
            next.push(poseidon2_hash(&[left, right]));
        }
        current = next;
    }
    current[0]
}

fn sha256_merkle_root(leaves: &[Fr]) -> Fr {
    if leaves.is_empty() {
        return Fr::zero();
    }
    if leaves.len() == 1 {
        return leaves[0];
    }

    let mut current = leaves.to_vec();
    while current.len() > 1 {
        let mut next = Vec::with_capacity(current.len().div_ceil(2));
        for chunk in current.chunks(2) {
            let left = chunk[0].to_be_bytes();
            let right = chunk.get(1).unwrap_or(&Fr::zero()).to_be_bytes();
            next.push(sha256_to_field(
                &[left.as_slice(), right.as_slice()].concat(),
            ));
        }
        current = next;
    }
    current[0]
}

/// Compute the SHA256 hash of a byte slice, returning the result as an `Fr`
/// (reduced mod the BN254 scalar field order).
fn sha256_to_field(data: &[u8]) -> Fr {
    let hash = Sha256::digest(data);
    Fr::from(<[u8; 32]>::try_from(hash.as_slice()).expect("SHA256 is 32 bytes"))
}

/// Compute the artifact hash for a contract.
///
/// Uses SHA256 for per-function bytecode/metadata hashing, then combines
/// private and unconstrained function artifact tree roots with a metadata hash.
pub fn compute_artifact_hash(artifact: &ContractArtifact) -> Fr {
    let private_fn_tree_root = compute_artifact_function_tree_root(artifact, false);
    let unconstrained_fn_tree_root = compute_artifact_function_tree_root(artifact, true);
    let metadata_hash = compute_artifact_metadata_hash(artifact);

    let mut data = Vec::new();
    data.push(1u8);
    data.extend_from_slice(&private_fn_tree_root.to_be_bytes());
    data.extend_from_slice(&unconstrained_fn_tree_root.to_be_bytes());
    data.extend_from_slice(&metadata_hash.to_be_bytes());
    sha256_to_field(&data)
}

fn canonical_json_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_owned(),
        serde_json::Value::Bool(boolean) => boolean.to_string(),
        serde_json::Value::Number(number) => number.to_string(),
        serde_json::Value::String(string) => {
            serde_json::to_string(string).unwrap_or_else(|_| "\"\"".to_owned())
        }
        serde_json::Value::Array(items) => {
            let inner = items
                .iter()
                .map(canonical_json_string)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{inner}]")
        }
        serde_json::Value::Object(map) => {
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            let inner = entries
                .into_iter()
                .map(|(key, value)| {
                    let key = serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_owned());
                    format!("{key}:{}", canonical_json_string(value))
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{inner}}}")
        }
    }
}

fn decode_artifact_bytes(encoded: &str) -> Vec<u8> {
    if let Some(hex) = encoded.strip_prefix("0x") {
        return hex::decode(hex).unwrap_or_else(|_| encoded.as_bytes().to_vec());
    }

    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap_or_else(|_| encoded.as_bytes().to_vec())
}

/// Compute the artifact function tree root for private or unconstrained functions.
fn compute_artifact_function_tree_root(artifact: &ContractArtifact, unconstrained: bool) -> Fr {
    let functions: Vec<&FunctionArtifact> = artifact
        .functions
        .iter()
        .filter(|f| {
            if unconstrained {
                f.function_type == FunctionType::Utility || f.is_unconstrained == Some(true)
            } else {
                f.function_type == FunctionType::Private
            }
        })
        .collect();

    if functions.is_empty() {
        return Fr::zero();
    }

    let leaves: Vec<Fr> = functions
        .iter()
        .map(|func| {
            let selector = func.selector.unwrap_or_else(|| {
                FunctionSelector::from_name_and_parameters(&func.name, &func.parameters)
            });
            let metadata_hash = compute_function_metadata_hash(func);
            let bytecode_hash = compute_function_bytecode_hash(func);

            let mut leaf_data = Vec::new();
            leaf_data.push(1u8);
            leaf_data.extend_from_slice(&selector.0);
            leaf_data.extend_from_slice(&metadata_hash.to_be_bytes());
            leaf_data.extend_from_slice(&bytecode_hash.to_be_bytes());
            sha256_to_field(&leaf_data)
        })
        .collect();

    let height = if leaves.len() <= 1 {
        0
    } else {
        (leaves.len() as f64).log2().ceil() as usize
    };
    let num_leaves = 1usize << height;
    let mut padded = leaves;
    padded.resize(num_leaves.max(1), Fr::zero());
    sha256_merkle_root(&padded)
}

/// Hash function metadata exactly as upstream does.
fn compute_function_metadata_hash(func: &FunctionArtifact) -> Fr {
    let metadata = serde_json::to_value(&func.return_types).unwrap_or(serde_json::Value::Null);
    let serialized = canonical_json_string(&metadata);
    sha256_to_field(serialized.as_bytes())
}

/// Hash function bytecode.
fn compute_function_bytecode_hash(func: &FunctionArtifact) -> Fr {
    match &func.bytecode {
        Some(bc) if !bc.is_empty() => sha256_to_field(&decode_artifact_bytes(bc)),
        _ => Fr::zero(),
    }
}

/// Hash artifact-level metadata.
fn compute_artifact_metadata_hash(artifact: &ContractArtifact) -> Fr {
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "name".to_owned(),
        serde_json::Value::String(artifact.name.clone()),
    );
    if let Some(outputs) = &artifact.outputs {
        metadata.insert("outputs".to_owned(), outputs.clone());
    }
    let serialized = canonical_json_string(&serde_json::Value::Object(metadata));
    sha256_to_field(serialized.as_bytes())
}

/// Compute the commitment to packed public bytecode.
///
/// Encodes bytecode as field elements (31 bytes each) and hashes with Poseidon2.
pub fn compute_public_bytecode_commitment(packed_bytecode: &[u8]) -> Fr {
    let fields = crate::abi::buffer_as_fields(
        packed_bytecode,
        constants::MAX_PACKED_PUBLIC_BYTECODE_SIZE_IN_FIELDS,
    )
    .expect("packed bytecode exceeds maximum field count");
    let byte_length = fields[0].to_usize() as u64;
    let length_in_fields = byte_length.div_ceil(31) as usize;

    let separator = Fr::from(u64::from(domain_separator::PUBLIC_BYTECODE) + (byte_length << 32));
    poseidon2_hash_with_separator_field(&fields[1..1 + length_in_fields], separator)
}

/// Compute the contract class ID from its components.
///
/// `class_id = poseidon2_hash_with_separator([artifact_hash, private_functions_root, public_bytecode_commitment], CONTRACT_CLASS_ID)`
pub fn compute_contract_class_id(
    artifact_hash: Fr,
    private_functions_root: Fr,
    public_bytecode_commitment: Fr,
) -> Fr {
    poseidon2_hash_with_separator(
        &[
            artifact_hash,
            private_functions_root,
            public_bytecode_commitment,
        ],
        domain_separator::CONTRACT_CLASS_ID,
    )
}

/// Compute contract class ID directly from a `ContractArtifact`.
pub fn compute_contract_class_id_from_artifact(artifact: &ContractArtifact) -> Result<Fr, Error> {
    let artifact_hash = compute_artifact_hash(artifact);
    let private_fns_root = compute_private_functions_root_from_artifact(artifact)?;
    let public_bytecode = extract_packed_public_bytecode(artifact);
    let public_bytecode_commitment = compute_public_bytecode_commitment(&public_bytecode);
    Ok(compute_contract_class_id(
        artifact_hash,
        private_fns_root,
        public_bytecode_commitment,
    ))
}

/// Extract private functions from an artifact and compute the root.
pub fn compute_private_functions_root_from_artifact(
    artifact: &ContractArtifact,
) -> Result<Fr, Error> {
    let mut private_fns: Vec<(FunctionSelector, Fr)> = artifact
        .functions
        .iter()
        .filter(|f| f.function_type == FunctionType::Private)
        .map(|f| {
            let selector = f.selector.unwrap_or_else(|| {
                FunctionSelector::from_name_and_parameters(&f.name, &f.parameters)
            });
            let vk_hash = f.verification_key_hash.unwrap_or(Fr::zero());
            (selector, vk_hash)
        })
        .collect();

    Ok(compute_private_functions_root(&mut private_fns))
}

/// Extract packed public bytecode from an artifact.
fn extract_packed_public_bytecode(artifact: &ContractArtifact) -> Vec<u8> {
    let mut bytecode = Vec::new();
    for func in &artifact.functions {
        if func.function_type == FunctionType::Public {
            if let Some(ref bc) = func.bytecode {
                bytecode.extend_from_slice(&decode_artifact_bytes(bc));
            }
        }
    }
    bytecode
}

// ---------------------------------------------------------------------------
// Contract address derivation (Step 5.6)
// ---------------------------------------------------------------------------

/// Compute the salted initialization hash.
///
/// `salted = poseidon2_hash_with_separator([salt, initialization_hash, deployer], PARTIAL_ADDRESS)`
pub fn compute_salted_initialization_hash(
    salt: Fr,
    initialization_hash: Fr,
    deployer: AztecAddress,
) -> Fr {
    poseidon2_hash_with_separator(
        &[salt, initialization_hash, deployer.0],
        domain_separator::PARTIAL_ADDRESS,
    )
}

/// Compute the partial address from class ID and salted init hash.
///
/// `partial = poseidon2_hash_with_separator([class_id, salted_init_hash], PARTIAL_ADDRESS)`
pub fn compute_partial_address(
    original_contract_class_id: Fr,
    salted_initialization_hash: Fr,
) -> Fr {
    poseidon2_hash_with_separator(
        &[original_contract_class_id, salted_initialization_hash],
        domain_separator::PARTIAL_ADDRESS,
    )
}

/// Compute an Aztec address from public keys and a partial address.
///
/// Algorithm:
///   1. `preaddress = poseidon2([public_keys_hash, partial_address], CONTRACT_ADDRESS_V1)`
///   2. `address_point = (Fq(preaddress) * G) + ivpk_m`
///   3. `address = address_point.x`
pub fn compute_address(
    public_keys: &PublicKeys,
    partial_address: &Fr,
) -> Result<AztecAddress, Error> {
    let public_keys_hash = public_keys.hash();
    let preaddress = poseidon2_hash_with_separator(
        &[public_keys_hash, *partial_address],
        domain_separator::CONTRACT_ADDRESS_V1,
    );

    // Convert Fr preaddress to Fq for Grumpkin scalar multiplication
    // (matches TS: `new Fq(preaddress.toBigInt())`)
    let preaddress_fq = Fq::from_be_bytes_mod_order(&preaddress.to_be_bytes());

    let g = grumpkin::generator();
    let preaddress_point = grumpkin::scalar_mul(&preaddress_fq, &g);

    let ivpk_m = &public_keys.master_incoming_viewing_public_key;
    // Point::is_zero() already checks !is_infinite, so no extra guard needed.
    let address_point = if ivpk_m.is_zero() {
        preaddress_point
    } else {
        grumpkin::point_add(&preaddress_point, ivpk_m)
    };

    if address_point.is_infinite {
        return Err(Error::InvalidData(
            "address derivation resulted in point at infinity".to_owned(),
        ));
    }

    Ok(AztecAddress(address_point.x))
}

/// Compute the contract address from a `ContractInstance`.
///
/// ```text
/// address = (poseidon2_hash_with_separator(
///     [public_keys_hash, partial_address],
///     CONTRACT_ADDRESS_V1
/// ) * G + ivpk_m).x
/// ```
pub fn compute_contract_address_from_instance(
    instance: &ContractInstance,
) -> Result<AztecAddress, Error> {
    let salted_init_hash = compute_salted_initialization_hash(
        instance.salt,
        instance.initialization_hash,
        instance.deployer,
    );
    let partial_address =
        compute_partial_address(instance.original_contract_class_id, salted_init_hash);

    compute_address(&instance.public_keys, &partial_address)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::abi::{buffer_as_fields, FunctionSelector, FunctionType};

    #[test]
    fn var_args_hash_empty_returns_zero() {
        assert_eq!(compute_var_args_hash(&[]), Fr::zero());
    }

    #[test]
    fn poseidon2_hash_known_vector() {
        // Test vector: hash of [1] should produce a known result.
        // This validates the sponge construction matches barretenberg.
        let result = poseidon2_hash(&[Fr::from(1u64)]);
        let expected =
            Fr::from_hex("0x168758332d5b3e2d13be8048c8011b454590e06c44bce7f702f09103eef5a373")
                .expect("valid hex");
        assert_eq!(
            result, expected,
            "Poseidon2 hash of [1] must match barretenberg test vector"
        );
    }

    #[test]
    fn poseidon2_hash_with_separator_prepends_separator() {
        // hash_with_separator([a, b], sep) == hash([Fr(sep), a, b])
        let a = Fr::from(10u64);
        let b = Fr::from(20u64);
        let sep = 42u32;

        let result = poseidon2_hash_with_separator(&[a, b], sep);
        let manual = poseidon2_hash(&[Fr::from(u64::from(sep)), a, b]);
        assert_eq!(result, manual);
    }

    #[test]
    fn secret_hash_uses_correct_separator() {
        let secret = Fr::from(42u64);
        let result = compute_secret_hash(&secret);
        let expected = poseidon2_hash_with_separator(&[secret], domain_separator::SECRET_HASH);
        assert_eq!(result, expected);
        // Must be non-zero for a non-zero secret
        assert!(!result.is_zero());
    }

    #[test]
    fn secret_hash_is_deterministic() {
        let secret = Fr::from(12345u64);
        let h1 = compute_secret_hash(&secret);
        let h2 = compute_secret_hash(&secret);
        assert_eq!(h1, h2);
    }

    #[test]
    fn var_args_hash_single_element() {
        let result = compute_var_args_hash(&[Fr::from(42u64)]);
        // Should be poseidon2_hash([FUNCTION_ARGS_SEP, 42])
        let expected =
            poseidon2_hash_with_separator(&[Fr::from(42u64)], domain_separator::FUNCTION_ARGS);
        assert_eq!(result, expected);
    }

    #[test]
    fn inner_auth_wit_hash_uses_correct_separator() {
        let args = [Fr::from(1u64), Fr::from(2u64), Fr::from(3u64)];
        let result = compute_inner_auth_wit_hash(&args);
        let expected = poseidon2_hash_with_separator(&args, domain_separator::AUTHWIT_INNER);
        assert_eq!(result, expected);
    }

    #[test]
    fn outer_auth_wit_hash_uses_correct_separator() {
        let consumer = AztecAddress(Fr::from(100u64));
        let chain_id = Fr::from(31337u64);
        let version = Fr::from(1u64);
        let inner_hash = Fr::from(999u64);

        let result = compute_outer_auth_wit_hash(&consumer, &chain_id, &version, &inner_hash);
        let expected = poseidon2_hash_with_separator(
            &[consumer.0, chain_id, version, inner_hash],
            domain_separator::AUTHWIT_OUTER,
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn inner_auth_wit_hash_from_action() {
        let caller = AztecAddress(Fr::from(1u64));
        let call = FunctionCall {
            to: AztecAddress(Fr::from(2u64)),
            selector: FunctionSelector::from_hex("0xaabbccdd").expect("valid"),
            args: vec![AbiValue::Field(Fr::from(100u64))],
            function_type: FunctionType::Private,
            is_static: false,
        };

        let result = compute_inner_auth_wit_hash_from_action(&caller, &call);

        // Manual computation
        let args_hash = compute_var_args_hash(&[Fr::from(100u64)]);
        let selector_field = call.selector.to_field();
        let expected = compute_inner_auth_wit_hash(&[caller.0, selector_field, args_hash]);
        assert_eq!(result, expected);
    }

    #[test]
    fn auth_wit_message_hash_passthrough() {
        let hash = Fr::from(42u64);
        let chain_info = ChainInfo {
            chain_id: Fr::from(31337u64),
            version: Fr::from(1u64),
        };
        let result =
            compute_auth_wit_message_hash(&MessageHashOrIntent::Hash { hash }, &chain_info);
        assert_eq!(result, hash);
    }

    #[test]
    fn auth_wit_message_hash_from_intent() {
        let caller = AztecAddress(Fr::from(10u64));
        let consumer = AztecAddress(Fr::from(20u64));
        let call = FunctionCall {
            to: consumer,
            selector: FunctionSelector::from_hex("0x11223344").expect("valid"),
            args: vec![],
            function_type: FunctionType::Private,
            is_static: false,
        };
        let chain_info = ChainInfo {
            chain_id: Fr::from(31337u64),
            version: Fr::from(1u64),
        };

        let result = compute_auth_wit_message_hash(
            &MessageHashOrIntent::Intent {
                caller,
                call: call.clone(),
            },
            &chain_info,
        );

        // Manual computation
        let inner = compute_inner_auth_wit_hash_from_action(&caller, &call);
        let expected = compute_outer_auth_wit_hash(
            &consumer,
            &chain_info.chain_id,
            &chain_info.version,
            &inner,
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn auth_wit_message_hash_from_inner_hash() {
        let consumer = AztecAddress(Fr::from(20u64));
        let inner_hash = Fr::from(999u64);
        let chain_info = ChainInfo {
            chain_id: Fr::from(31337u64),
            version: Fr::from(1u64),
        };

        let result = compute_auth_wit_message_hash(
            &MessageHashOrIntent::InnerHash {
                consumer,
                inner_hash,
            },
            &chain_info,
        );

        let expected = compute_outer_auth_wit_hash(
            &consumer,
            &chain_info.chain_id,
            &chain_info.version,
            &inner_hash,
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn abi_values_to_fields_basic_types() {
        let values = vec![
            AbiValue::Field(Fr::from(1u64)),
            AbiValue::Boolean(true),
            AbiValue::Boolean(false),
            AbiValue::Integer(42),
        ];
        let fields = abi_values_to_fields(&values);
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0], Fr::from(1u64));
        assert_eq!(fields[1], Fr::one());
        assert_eq!(fields[2], Fr::zero());
        assert_eq!(fields[3], Fr::from(42u64));
    }

    #[test]
    fn abi_values_to_fields_nested() {
        let values = vec![AbiValue::Array(vec![
            AbiValue::Field(Fr::from(1u64)),
            AbiValue::Field(Fr::from(2u64)),
        ])];
        let fields = abi_values_to_fields(&values);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0], Fr::from(1u64));
        assert_eq!(fields[1], Fr::from(2u64));
    }

    #[test]
    fn message_hash_or_intent_serde_roundtrip() {
        let variants = vec![
            MessageHashOrIntent::Hash {
                hash: Fr::from(42u64),
            },
            MessageHashOrIntent::Intent {
                caller: AztecAddress(Fr::from(1u64)),
                call: FunctionCall {
                    to: AztecAddress(Fr::from(2u64)),
                    selector: FunctionSelector::from_hex("0xaabbccdd").expect("valid"),
                    args: vec![],
                    function_type: FunctionType::Private,
                    is_static: false,
                },
            },
            MessageHashOrIntent::InnerHash {
                consumer: AztecAddress(Fr::from(3u64)),
                inner_hash: Fr::from(999u64),
            },
        ];

        for variant in variants {
            let json = serde_json::to_string(&variant).expect("serialize");
            let decoded: MessageHashOrIntent = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(decoded, variant);
        }
    }

    #[test]
    fn chain_info_serde_roundtrip() {
        let info = ChainInfo {
            chain_id: Fr::from(31337u64),
            version: Fr::from(1u64),
        };
        let json = serde_json::to_string(&info).expect("serialize");
        let decoded: ChainInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, info);
    }

    // -- Deployment hash tests --

    #[test]
    fn initialization_hash_no_constructor_returns_zero() {
        let result = compute_initialization_hash(None, &[]).expect("no constructor");
        assert_eq!(result, Fr::zero());
    }

    #[test]
    fn initialization_hash_with_constructor() {
        use crate::abi::AbiParameter;
        let func = FunctionArtifact {
            name: "constructor".to_owned(),
            function_type: FunctionType::Private,
            is_initializer: true,
            is_static: false,
            is_only_self: None,
            parameters: vec![AbiParameter {
                name: "admin".to_owned(),
                typ: crate::abi::AbiType::Field,
                visibility: None,
            }],
            return_types: vec![],
            error_types: None,
            selector: Some(FunctionSelector::from_hex("0xe5fb6c81").expect("valid")),
            bytecode: None,
            verification_key_hash: None,
            verification_key: None,
            custom_attributes: None,
            is_unconstrained: None,
            debug_symbols: None,
        };
        let args = vec![AbiValue::Field(Fr::from(42u64))];
        let result = compute_initialization_hash(Some(&func), &args).expect("init hash");
        assert_ne!(result, Fr::zero());
    }

    #[test]
    fn initialization_hash_from_encoded() {
        let selector = Fr::from(12345u64);
        let args = vec![Fr::from(1u64), Fr::from(2u64)];
        let result = compute_initialization_hash_from_encoded(selector, &args);
        let args_hash = compute_var_args_hash(&args);
        let expected =
            poseidon2_hash_with_separator(&[selector, args_hash], domain_separator::INITIALIZER);
        assert_eq!(result, expected);
    }

    #[test]
    fn private_functions_root_empty() {
        let root = compute_private_functions_root(&mut []);
        // Empty leaves all zero => root is the Merkle root of 128 zero leaves
        assert_ne!(root, Fr::zero()); // still a valid root, just all-zero tree
    }

    #[test]
    fn contract_class_id_deterministic() {
        let artifact_hash = Fr::from(1u64);
        let root = Fr::from(2u64);
        let commitment = Fr::from(3u64);
        let id1 = compute_contract_class_id(artifact_hash, root, commitment);
        let id2 = compute_contract_class_id(artifact_hash, root, commitment);
        assert_eq!(id1, id2);
        assert_ne!(id1, Fr::zero());
    }

    #[test]
    fn buffer_as_fields_basic() {
        let data = vec![0u8; 31];
        let fields = buffer_as_fields(&data, 100).expect("encode");
        // Result is padded to max_fields; first field is the length prefix,
        // second is the single 31-byte chunk, rest are zero-padding.
        assert_eq!(fields.len(), 100);
        assert_eq!(fields[0], Fr::from(31u64)); // length prefix
    }

    #[test]
    fn buffer_as_fields_multiple_chunks() {
        let data = vec![0xffu8; 62]; // 2 chunks of 31 bytes
        let fields = buffer_as_fields(&data, 100).expect("encode");
        assert_eq!(fields.len(), 100);
        assert_eq!(fields[0], Fr::from(62u64)); // length prefix
    }

    #[test]
    fn public_bytecode_commitment_empty() {
        let result = compute_public_bytecode_commitment(&[]);
        // Even with empty bytecode the Poseidon2 hash with separator is non-zero.
        assert_ne!(result, Fr::zero());
    }

    #[test]
    fn public_bytecode_commitment_non_empty() {
        let data = vec![0x01u8; 100];
        let result = compute_public_bytecode_commitment(&data);
        assert_ne!(result, Fr::zero());
    }

    #[test]
    fn salted_initialization_hash_uses_partial_address_separator() {
        let salt = Fr::from(1u64);
        let init_hash = Fr::from(2u64);
        let deployer = AztecAddress(Fr::from(3u64));
        let result = compute_salted_initialization_hash(salt, init_hash, deployer);
        let expected = poseidon2_hash_with_separator(
            &[salt, init_hash, deployer.0],
            domain_separator::PARTIAL_ADDRESS,
        );
        assert_eq!(result, expected);
    }

    #[test]
    fn partial_address_uses_correct_separator() {
        let class_id = Fr::from(100u64);
        let salted = Fr::from(200u64);
        let result = compute_partial_address(class_id, salted);
        let expected =
            poseidon2_hash_with_separator(&[class_id, salted], domain_separator::PARTIAL_ADDRESS);
        assert_eq!(result, expected);
    }

    #[test]
    fn contract_address_from_instance_default_keys() {
        use crate::types::{ContractInstance, PublicKeys};
        let instance = ContractInstance {
            version: 1,
            salt: Fr::from(42u64),
            deployer: AztecAddress(Fr::zero()),
            current_contract_class_id: Fr::from(100u64),
            original_contract_class_id: Fr::from(100u64),
            initialization_hash: Fr::zero(),
            public_keys: PublicKeys::default(),
        };
        let address =
            compute_contract_address_from_instance(&instance).expect("address derivation");
        assert_ne!(address.0, Fr::zero());
    }

    #[test]
    fn contract_address_is_deterministic() {
        use crate::types::{ContractInstance, PublicKeys};
        let instance = ContractInstance {
            version: 1,
            salt: Fr::from(99u64),
            deployer: AztecAddress(Fr::from(1u64)),
            current_contract_class_id: Fr::from(200u64),
            original_contract_class_id: Fr::from(200u64),
            initialization_hash: Fr::from(300u64),
            public_keys: PublicKeys::default(),
        };
        let addr1 = compute_contract_address_from_instance(&instance).expect("addr1");
        let addr2 = compute_contract_address_from_instance(&instance).expect("addr2");
        assert_eq!(addr1, addr2);
    }

    #[test]
    fn artifact_hash_deterministic() {
        let artifact = ContractArtifact {
            name: "Test".to_owned(),
            functions: vec![],
            outputs: None,
            file_map: None,
        };
        let h1 = compute_artifact_hash(&artifact);
        let h2 = compute_artifact_hash(&artifact);
        assert_eq!(h1, h2);
    }

    #[test]
    fn class_id_from_artifact_no_functions() {
        let artifact = ContractArtifact {
            name: "Empty".to_owned(),
            functions: vec![],
            outputs: None,
            file_map: None,
        };
        let id = compute_contract_class_id_from_artifact(&artifact).expect("class id");
        assert_ne!(id, Fr::zero());
    }
}
