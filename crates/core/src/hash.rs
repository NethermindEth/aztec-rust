//! Poseidon2 hash functions for the Aztec protocol.
//!
//! Provides `poseidon2_hash_with_separator` and derived functions that mirror the
//! TypeScript SDK's hashing utilities.

use crate::abi::AbiValue;
use crate::constants::domain_separator;
use crate::tx::FunctionCall;
use crate::types::{AztecAddress, Fr};

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
    let mut full_input = Vec::with_capacity(1 + inputs.len());
    full_input.push(Fr::from(u64::from(separator)));
    full_input.extend_from_slice(inputs);
    poseidon2_hash(&full_input)
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::abi::{FunctionSelector, FunctionType};

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
}
