//! Well-known protocol contract addresses and domain separators.
//!
//! These addresses and constants are deterministic and identical across all Aztec networks.

use crate::types::{AztecAddress, Fr};

/// Well-known protocol contract addresses.
pub mod protocol_contract_address {
    use super::*;

    /// The Fee Juice contract — manages fee token balances and claims.
    ///
    /// In the TS SDK this is `ProtocolContractAddress.FeeJuice` with
    /// the numeric value `5` (see `constants.gen.ts: FEE_JUICE_ADDRESS = 5`).
    pub fn fee_juice() -> AztecAddress {
        AztecAddress(Fr::from(5u64))
    }

    /// The AuthRegistry protocol contract — manages public authorization witnesses.
    ///
    /// In the TS SDK this is `ProtocolContractAddress.AuthRegistry` with
    /// the numeric value `1` (see `constants.gen.ts: CANONICAL_AUTH_REGISTRY_ADDRESS = 1`).
    pub fn auth_registry() -> AztecAddress {
        AztecAddress(Fr::from(1u64))
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
    }
}
