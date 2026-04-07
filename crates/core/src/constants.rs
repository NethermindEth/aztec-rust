//! Well-known protocol contract addresses and domain separators.
//!
//! These addresses and constants are deterministic and identical across all Aztec networks.

use crate::types::{AztecAddress, Fr};

/// Well-known protocol contract addresses.
pub mod protocol_contract_address {
    use super::*;

    /// The Fee Juice contract — manages fee token balances and claims.
    pub fn fee_juice() -> AztecAddress {
        AztecAddress(Fr::from(5u64))
    }

    /// The AuthRegistry protocol contract — manages public authorization witnesses.
    pub fn auth_registry() -> AztecAddress {
        AztecAddress(Fr::from(1u64))
    }

    /// The Contract Instance Deployer — registers contract instances on-chain.
    pub fn contract_instance_deployer() -> AztecAddress {
        AztecAddress(Fr::from(2u64))
    }

    /// The Contract Instance Registry — canonical public deployment registry.
    pub fn contract_instance_registry() -> AztecAddress {
        contract_instance_deployer()
    }

    /// The Contract Class Registerer — publishes contract classes on-chain.
    pub fn contract_class_registerer() -> AztecAddress {
        AztecAddress(Fr::from(3u64))
    }

    /// The Contract Class Registry — canonical class publication registry.
    pub fn contract_class_registry() -> AztecAddress {
        contract_class_registerer()
    }

    /// The Multi-Call Entrypoint — batches multiple calls in one tx.
    pub fn multi_call_entrypoint() -> AztecAddress {
        AztecAddress(Fr::from(4u64))
    }
}

/// Domain separators used in Poseidon2 hashing throughout the protocol.
///
/// These must match the TS constants in `constants.gen.ts`.
pub mod domain_separator {
    /// Domain separator for authwit inner hash.
    ///
    /// TS: `DomainSeparator.AUTHWIT_INNER = 221354163`
    pub const AUTHWIT_INNER: u32 = 221_354_163;

    /// Domain separator for authwit outer hash.
    ///
    /// TS: `DomainSeparator.AUTHWIT_OUTER = 3283595782`
    pub const AUTHWIT_OUTER: u32 = 3_283_595_782;

    /// Domain separator for function args hashing.
    ///
    /// TS: `DomainSeparator.FUNCTION_ARGS = 3576554347`
    pub const FUNCTION_ARGS: u32 = 3_576_554_347;

    /// Domain separator for public keys hash computation.
    pub const PUBLIC_KEYS_HASH: u32 = 777_457_226;

    /// Domain separator for partial address / salted initialization hash.
    pub const PARTIAL_ADDRESS: u32 = 2_103_633_018;

    /// Domain separator for contract class ID computation.
    pub const CONTRACT_CLASS_ID: u32 = 3_923_495_515;

    /// Domain separator for private function leaf hashing.
    pub const PRIVATE_FUNCTION_LEAF: u32 = 1_389_398_688;

    /// Domain separator for public bytecode commitment.
    pub const PUBLIC_BYTECODE: u32 = 260_313_585;

    /// Domain separator for initialization hash computation.
    pub const INITIALIZER: u32 = 385_396_519;

    /// Domain separator for contract address V1 derivation.
    pub const CONTRACT_ADDRESS_V1: u32 = 1_788_365_517;
}

/// Size constants for deployment computations.

/// Height of the private functions Merkle tree.
pub const FUNCTION_TREE_HEIGHT: usize = 7;

/// Maximum number of field elements in packed public bytecode.
pub const MAX_PACKED_PUBLIC_BYTECODE_SIZE_IN_FIELDS: usize = 3000;

/// Maximum height of the artifact function tree.
pub const ARTIFACT_FUNCTION_TREE_MAX_HEIGHT: usize = 7;

/// Maximum processable L2 gas for a transaction.
pub const MAX_PROCESSABLE_L2_GAS: u64 = 6_540_000;

/// Bytecode capsule slot used by the Contract Class Registry.
pub fn contract_class_registry_bytecode_capsule_slot() -> Fr {
    Fr::from_hex("0x1f61038721b052d5389449bf44f73c817146aedfab1ef13d37f16ce928df1fb7")
        .expect("valid contract class registry capsule slot constant")
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn fee_juice_address_is_5() {
        let addr = protocol_contract_address::fee_juice();
        assert_eq!(addr, AztecAddress(Fr::from(5u64)));
    }

    #[test]
    fn auth_registry_address_is_1() {
        let addr = protocol_contract_address::auth_registry();
        assert_eq!(addr, AztecAddress(Fr::from(1u64)));
    }

    #[test]
    fn domain_separator_values_match_ts() {
        assert_eq!(domain_separator::AUTHWIT_INNER, 221_354_163);
        assert_eq!(domain_separator::AUTHWIT_OUTER, 3_283_595_782);
        assert_eq!(domain_separator::FUNCTION_ARGS, 3_576_554_347);
        assert_eq!(domain_separator::PUBLIC_KEYS_HASH, 777_457_226);
        assert_eq!(domain_separator::PARTIAL_ADDRESS, 2_103_633_018);
        assert_eq!(domain_separator::CONTRACT_CLASS_ID, 3_923_495_515);
        assert_eq!(domain_separator::PRIVATE_FUNCTION_LEAF, 1_389_398_688);
        assert_eq!(domain_separator::PUBLIC_BYTECODE, 260_313_585);
        assert_eq!(domain_separator::INITIALIZER, 385_396_519);
        assert_eq!(domain_separator::CONTRACT_ADDRESS_V1, 1_788_365_517);
    }

    #[test]
    fn protocol_contract_addresses() {
        assert_eq!(
            protocol_contract_address::contract_instance_deployer(),
            AztecAddress(Fr::from(2u64))
        );
        assert_eq!(
            protocol_contract_address::contract_class_registerer(),
            AztecAddress(Fr::from(3u64))
        );
        assert_eq!(
            protocol_contract_address::multi_call_entrypoint(),
            AztecAddress(Fr::from(4u64))
        );
    }

    #[test]
    fn size_constants() {
        assert_eq!(super::FUNCTION_TREE_HEIGHT, 7);
        assert_eq!(super::MAX_PACKED_PUBLIC_BYTECODE_SIZE_IN_FIELDS, 3000);
        assert_eq!(super::ARTIFACT_FUNCTION_TREE_MAX_HEIGHT, 7);
        assert_eq!(super::MAX_PROCESSABLE_L2_GAS, 6_540_000);
    }

    #[test]
    fn capsule_slot_constant_matches_ts() {
        assert_eq!(
            contract_class_registry_bytecode_capsule_slot(),
            Fr::from_hex("0x1f61038721b052d5389449bf44f73c817146aedfab1ef13d37f16ce928df1fb7")
                .expect("valid slot")
        );
    }
}
