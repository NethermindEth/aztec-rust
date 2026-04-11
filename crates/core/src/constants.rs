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

    /// The PublicChecks protocol contract.
    pub fn public_checks() -> AztecAddress {
        AztecAddress(Fr::from(6u64))
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

    /// Sentinel msg sender address used by protocol nullifiers.
    ///
    /// Upstream `NULL_MSG_SENDER_CONTRACT_ADDRESS = AztecAddress::from_field(-1)`.
    pub fn null_msg_sender() -> AztecAddress {
        AztecAddress(
            Fr::from_hex("0x30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000000")
                .expect("valid null msg sender address"),
        )
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

    /// Domain separator for public function calldata hashing.
    ///
    /// TS: `DomainSeparator.PUBLIC_CALLDATA = 2760353947`
    pub const PUBLIC_CALLDATA: u32 = 2_760_353_947;
    /// Domain separator for private tx hashes.
    ///
    /// TS: `DomainSeparator.PRIVATE_TX_HASH = 1971680439`
    pub const PRIVATE_TX_HASH: u32 = 1_971_680_439;
    /// Domain separator for public tx hashes.
    ///
    /// TS: `DomainSeparator.PUBLIC_TX_HASH = 1630108851`
    pub const PUBLIC_TX_HASH: u32 = 1_630_108_851;

    /// Domain separator for tx request hashes.
    ///
    /// TS: `DomainSeparator.TX_REQUEST = 3763737512`
    pub const TX_REQUEST: u32 = 3_763_737_512;

    /// Domain separator for the protocol contracts tuple hash.
    ///
    /// TS: `DomainSeparator.PROTOCOL_CONTRACTS = 3904434327`
    pub const PROTOCOL_CONTRACTS: u32 = 3_904_434_327;

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

    /// Master nullifier hiding key derivation.
    ///
    /// TS: `DomainSeparator.NHK_M = 242137788`
    pub const NHK_M: u32 = 242_137_788;

    /// Master incoming viewing secret key derivation.
    ///
    /// TS: `DomainSeparator.IVSK_M = 2747825907`
    pub const IVSK_M: u32 = 2_747_825_907;

    /// Master outgoing viewing secret key derivation.
    ///
    /// TS: `DomainSeparator.OVSK_M = 4272201051`
    pub const OVSK_M: u32 = 4_272_201_051;

    /// Master tagging secret key derivation.
    ///
    /// TS: `DomainSeparator.TSK_M = 1546190975`
    pub const TSK_M: u32 = 1_546_190_975;

    /// Secret hash (for L1-L2 messages and TransparentNote).
    ///
    /// TS: `DomainSeparator.SECRET_HASH = 4199652938`
    pub const SECRET_HASH: u32 = 4_199_652_938;

    /// Domain separator for signature payload hashing (entrypoint encoding).
    ///
    /// TS: `DomainSeparator.SIGNATURE_PAYLOAD = 463525807`
    pub const SIGNATURE_PAYLOAD: u32 = 463_525_807;

    /// Domain separator for siloing note hashes with a contract address.
    ///
    /// TS: `DomainSeparator.SILO_NOTE_HASH = 1864988894`
    pub const SILO_NOTE_HASH: u32 = 1_864_988_894;

    /// Domain separator for siloing nullifiers with a contract address.
    ///
    /// TS: `DomainSeparator.SILO_NULLIFIER = 3956568061`
    pub const SILO_NULLIFIER: u32 = 3_956_568_061;

    /// Domain separator for unique note hash computation.
    pub const UNIQUE_NOTE_HASH: u32 = 226_850_429;

    /// Domain separator for note hash nonce computation.
    pub const NOTE_HASH_NONCE: u32 = 1_721_808_740;

    /// Domain separator for siloed note hash (inner silo step).
    pub const SILOED_NOTE_HASH: u32 = 3_361_878_420;

    /// Domain separator for siloed nullifier (inner silo step).
    pub const SILOED_NULLIFIER: u32 = 57_496_191;

    /// Domain separator for private log first field siloing.
    pub const PRIVATE_LOG_FIRST_FIELD: u32 = 2_769_976_252;

    /// Domain separator for note nullifier derivation.
    ///
    /// TS: `DomainSeparator.NOTE_NULLIFIER = 50789342`
    pub const NOTE_NULLIFIER: u32 = 50_789_342;
}

// ---------------------------------------------------------------------------
// Per-transaction limits (from constants.gen.ts)
// ---------------------------------------------------------------------------

/// Maximum note hashes per transaction.
pub const MAX_NOTE_HASHES_PER_TX: usize = 64;
/// Maximum nullifiers per transaction.
pub const MAX_NULLIFIERS_PER_TX: usize = 64;
/// Maximum private logs per transaction.
pub const MAX_PRIVATE_LOGS_PER_TX: usize = 64;
/// Maximum L2-to-L1 messages per transaction.
pub const MAX_L2_TO_L1_MSGS_PER_TX: usize = 8;
/// Maximum enqueued public calls per transaction.
pub const MAX_ENQUEUED_CALLS_PER_TX: usize = 32;
/// Maximum contract class logs per transaction.
pub const MAX_CONTRACT_CLASS_LOGS_PER_TX: usize = 1;
/// Size of a contract class log in field elements.
pub const CONTRACT_CLASS_LOG_SIZE_IN_FIELDS: usize = 3023;

// Per-call limits
/// Maximum note hashes per call.
pub const MAX_NOTE_HASHES_PER_CALL: usize = 16;
/// Maximum nullifiers per call.
pub const MAX_NULLIFIERS_PER_CALL: usize = 16;
/// Maximum private call stack length per call.
pub const MAX_PRIVATE_CALL_STACK_LENGTH_PER_CALL: usize = 8;
/// Maximum enqueued calls per call.
pub const MAX_ENQUEUED_CALLS_PER_CALL: usize = 32;
/// Maximum L2-to-L1 messages per call.
pub const MAX_L2_TO_L1_MSGS_PER_CALL: usize = 8;
/// Maximum private logs per call.
pub const MAX_PRIVATE_LOGS_PER_CALL: usize = 16;
/// Maximum contract class logs per call.
pub const MAX_CONTRACT_CLASS_LOGS_PER_CALL: usize = 1;

// Read request limits
/// Maximum note hash read requests per transaction.
pub const MAX_NOTE_HASH_READ_REQUESTS_PER_TX: usize = 64;
/// Maximum nullifier read requests per transaction.
pub const MAX_NULLIFIER_READ_REQUESTS_PER_TX: usize = 64;
/// Maximum key validation requests per transaction.
pub const MAX_KEY_VALIDATION_REQUESTS_PER_TX: usize = 64;
/// Maximum note hash read requests per call.
pub const MAX_NOTE_HASH_READ_REQUESTS_PER_CALL: usize = 16;
/// Maximum nullifier read requests per call.
pub const MAX_NULLIFIER_READ_REQUESTS_PER_CALL: usize = 16;
/// Maximum key validation requests per call.
pub const MAX_KEY_VALIDATION_REQUESTS_PER_CALL: usize = 16;

/// Maximum calldata fields across all enqueued public calls.
pub const MAX_FR_CALLDATA_TO_ALL_ENQUEUED_CALLS: usize = 12_288;

// Private log size
/// Size of a private log in field elements.
pub const PRIVATE_LOG_SIZE_IN_FIELDS: usize = 16;

// ---------------------------------------------------------------------------
// Tree heights
// ---------------------------------------------------------------------------

/// Note hash tree height.
pub const NOTE_HASH_TREE_HEIGHT: usize = 42;
/// Nullifier tree height.
pub const NULLIFIER_TREE_HEIGHT: usize = 42;
/// Public data tree height.
pub const PUBLIC_DATA_TREE_HEIGHT: usize = 40;
/// L1-to-L2 message tree height.
pub const L1_TO_L2_MSG_TREE_HEIGHT: usize = 36;
/// Archive tree height.
pub const ARCHIVE_HEIGHT: usize = 30;
/// VK tree height.
pub const VK_TREE_HEIGHT: usize = 8;
/// Canonical protocol contract tuple length.
pub const MAX_PROTOCOL_CONTRACTS: usize = 11;

/// Canonical VK tree root for the pinned Aztec 4.1.3 protocol artifacts.
///
/// This matches `aztec compute-genesis-values` and upstream
/// `getVKTreeRoot()` from `aztec-packages`.
pub fn current_vk_tree_root() -> Fr {
    Fr::from_hex("0x1dd2644a17d1ddd8831287a78c5a1033b7ae35cdf2a3db833608856c062fc2ba")
        .expect("valid canonical VK tree root")
}

// ---------------------------------------------------------------------------
// Proof lengths
// ---------------------------------------------------------------------------

/// ChonkProof field count.
pub const CHONK_PROOF_LENGTH: usize = 1935;
/// Recursive proof field count.
pub const RECURSIVE_PROOF_LENGTH: usize = 449;

// ---------------------------------------------------------------------------
// Gas constants
// ---------------------------------------------------------------------------

/// DA gas overhead per transaction.
pub const TX_DA_GAS_OVERHEAD: u64 = 96;
/// L2 gas overhead for transactions with public calls.
pub const PUBLIC_TX_L2_GAS_OVERHEAD: u64 = 540_000;
/// L2 gas overhead for private-only transactions.
pub const PRIVATE_TX_L2_GAS_OVERHEAD: u64 = 440_000;
/// Fixed AVM startup L2 gas.
pub const FIXED_AVM_STARTUP_L2_GAS: u64 = 20_000;

/// L2 gas per note hash.
pub const L2_GAS_PER_NOTE_HASH: u64 = 9_200;
/// L2 gas per nullifier.
pub const L2_GAS_PER_NULLIFIER: u64 = 16_000;
/// L2 gas per L2-to-L1 message.
pub const L2_GAS_PER_L2_TO_L1_MSG: u64 = 5_200;
/// L2 gas per private log.
pub const L2_GAS_PER_PRIVATE_LOG: u64 = 2_500;
/// L2 gas per contract class log.
pub const L2_GAS_PER_CONTRACT_CLASS_LOG: u64 = 73_000;

/// Bytes per field element for DA cost.
pub const DA_BYTES_PER_FIELD: u64 = 32;
/// DA gas per byte.
pub const DA_GAS_PER_BYTE: u64 = 1;
/// DA gas per field element.
pub const DA_GAS_PER_FIELD: u64 = 32;

/// Maximum processable L2 gas.
pub const MAX_PROCESSABLE_L2_GAS: u64 = 6_540_000;
/// Maximum processable DA gas per checkpoint.
pub const MAX_PROCESSABLE_DA_GAS_PER_CHECKPOINT: u64 = 786_432;

/// Default L2 gas limit.
pub const DEFAULT_L2_GAS_LIMIT: u64 = 6_540_000;
/// Default teardown L2 gas limit.
pub const DEFAULT_TEARDOWN_L2_GAS_LIMIT: u64 = 1_000_000;
/// Default DA gas limit.
pub const DEFAULT_DA_GAS_LIMIT: u64 = 786_432;
/// Default teardown DA gas limit.
pub const DEFAULT_TEARDOWN_DA_GAS_LIMIT: u64 = 393_216;

/// Maximum transaction lifetime in seconds.
pub const MAX_TX_LIFETIME: u64 = 86_400;

// ---------------------------------------------------------------------------
// Size constants for deployment computations
// ---------------------------------------------------------------------------

/// Height of the private functions Merkle tree.
pub const FUNCTION_TREE_HEIGHT: usize = 7;

/// Maximum number of field elements in packed public bytecode.
pub const MAX_PACKED_PUBLIC_BYTECODE_SIZE_IN_FIELDS: usize = 3000;

/// Magic prefix for `ContractClassRegistry` emitted class-publication logs.
pub fn contract_class_published_magic_value() -> Fr {
    Fr::from_hex("0x20f5895a4e837356c2d551743df6bf642756dcd93cd31cbd37c556c90bf7f244")
        .expect("valid contract class published magic value")
}

/// Magic prefix for `ContractInstanceRegistry` emitted instance-publication logs.
pub fn contract_instance_published_magic_value() -> Fr {
    Fr::from_hex("0x174c6b3d0fd14728e4fc5e53f7b262ab943546a7e125e2ed5e9fde3cf0b3e22f")
        .expect("valid contract instance published magic value")
}

/// Maximum height of the artifact function tree.
pub const ARTIFACT_FUNCTION_TREE_MAX_HEIGHT: usize = 7;

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
        assert_eq!(domain_separator::PUBLIC_CALLDATA, 2_760_353_947);
        assert_eq!(domain_separator::PUBLIC_KEYS_HASH, 777_457_226);
        assert_eq!(domain_separator::PARTIAL_ADDRESS, 2_103_633_018);
        assert_eq!(domain_separator::CONTRACT_CLASS_ID, 3_923_495_515);
        assert_eq!(domain_separator::PRIVATE_FUNCTION_LEAF, 1_389_398_688);
        assert_eq!(domain_separator::PUBLIC_BYTECODE, 260_313_585);
        assert_eq!(domain_separator::INITIALIZER, 385_396_519);
        assert_eq!(domain_separator::CONTRACT_ADDRESS_V1, 1_788_365_517);
        // Key derivation separators
        assert_eq!(domain_separator::NHK_M, 242_137_788);
        assert_eq!(domain_separator::IVSK_M, 2_747_825_907);
        assert_eq!(domain_separator::OVSK_M, 4_272_201_051);
        assert_eq!(domain_separator::TSK_M, 1_546_190_975);
        assert_eq!(domain_separator::SECRET_HASH, 4_199_652_938);
        assert_eq!(domain_separator::SIGNATURE_PAYLOAD, 463_525_807);
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
    fn tx_limit_constants() {
        assert_eq!(super::MAX_NOTE_HASHES_PER_TX, 64);
        assert_eq!(super::MAX_NULLIFIERS_PER_TX, 64);
        assert_eq!(super::MAX_PRIVATE_LOGS_PER_TX, 64);
        assert_eq!(super::MAX_L2_TO_L1_MSGS_PER_TX, 8);
        assert_eq!(super::MAX_ENQUEUED_CALLS_PER_TX, 32);
        assert_eq!(super::MAX_CONTRACT_CLASS_LOGS_PER_TX, 1);
        assert_eq!(super::CONTRACT_CLASS_LOG_SIZE_IN_FIELDS, 3023);
        assert_eq!(super::CHONK_PROOF_LENGTH, 1935);
    }

    #[test]
    fn gas_constants() {
        assert_eq!(super::L2_GAS_PER_NOTE_HASH, 9_200);
        assert_eq!(super::L2_GAS_PER_NULLIFIER, 16_000);
        assert_eq!(super::DA_GAS_PER_FIELD, 32);
        assert_eq!(super::PRIVATE_TX_L2_GAS_OVERHEAD, 440_000);
        assert_eq!(super::PUBLIC_TX_L2_GAS_OVERHEAD, 540_000);
    }

    #[test]
    fn kernel_domain_separators() {
        assert_eq!(domain_separator::UNIQUE_NOTE_HASH, 226_850_429);
        assert_eq!(domain_separator::NOTE_HASH_NONCE, 1_721_808_740);
        assert_eq!(domain_separator::SILOED_NOTE_HASH, 3_361_878_420);
        assert_eq!(domain_separator::SILOED_NULLIFIER, 57_496_191);
        assert_eq!(domain_separator::PRIVATE_LOG_FIRST_FIELD, 2_769_976_252);
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
