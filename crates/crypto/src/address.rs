//! Address derivation utilities.

use aztec_core::hash::compute_address;
use aztec_core::types::{CompleteAddress, Fr};
use aztec_core::Error;

use crate::keys::derive_keys;

/// Derive a complete address from a secret key and partial address.
pub fn complete_address_from_secret_key_and_partial_address(
    secret_key: &Fr,
    partial_address: &Fr,
) -> Result<CompleteAddress, Error> {
    let derived = derive_keys(secret_key);
    let address = compute_address(&derived.public_keys, partial_address)?;
    Ok(CompleteAddress {
        address,
        public_keys: derived.public_keys,
        partial_address: *partial_address,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use aztec_core::types::AztecAddress;

    #[test]
    fn complete_address_from_secret_key() {
        let sk = Fr::from(8923u64);
        let partial_address = Fr::from(243523u64);
        let complete = complete_address_from_secret_key_and_partial_address(&sk, &partial_address)
            .expect("address derivation");
        let expected = AztecAddress(
            Fr::from_hex("0x2e54c8067c410d03d417dddd51e1cad76cece48ff39fa0fe908782b93a209a52")
                .expect("valid hex"),
        );
        assert_eq!(complete.address, expected);
        assert_eq!(complete.partial_address, partial_address);
        assert!(!complete.public_keys.is_empty());
    }
}
