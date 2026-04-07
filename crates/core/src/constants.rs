//! Well-known protocol contract addresses.
//!
//! These addresses are deterministic and identical across all Aztec networks.

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
}
