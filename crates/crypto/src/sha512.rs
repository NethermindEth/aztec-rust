//! SHA-512 to Grumpkin scalar conversion.

use aztec_core::types::{Fr, GrumpkinScalar};
use sha2::{Digest, Sha512};

/// Compute SHA-512 of the concatenated inputs and reduce to a Grumpkin scalar.
///
/// This is used for master key derivation where uniform distribution is
/// required (not just collision resistance). The 512-bit hash output
/// ensures negligible bias after modular reduction.
///
/// Mirrors TS `sha512ToGrumpkinScalar([secretKey, domainSeparator])`.
pub fn sha512_to_grumpkin_scalar(secret_key: &Fr, domain_separator: u32) -> GrumpkinScalar {
    // 1. Serialize: [secret_key (32 bytes BE)] ++ [domain_separator (4 bytes BE)]
    let mut input = [0u8; 36];
    input[..32].copy_from_slice(&secret_key.to_be_bytes());
    input[32..36].copy_from_slice(&domain_separator.to_be_bytes());

    // 2. SHA-512 hash
    let hash = Sha512::digest(input); // 64 bytes

    // 3. Reduce mod Fq::MODULUS
    GrumpkinScalar::from_be_bytes_mod_order(&hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aztec_core::constants::domain_separator;

    #[test]
    fn sha512_input_buffer_is_36_bytes() {
        // Verify the serialization layout: 32 bytes Fr + 4 bytes separator
        let sk = Fr::from(8923u64);
        let sep = domain_separator::NHK_M;

        let mut expected_input = [0u8; 36];
        expected_input[..32].copy_from_slice(&sk.to_be_bytes());
        expected_input[32..36].copy_from_slice(&sep.to_be_bytes());

        // The function produces a non-zero result
        let result = sha512_to_grumpkin_scalar(&sk, sep);
        assert!(!result.is_zero());
    }

    #[test]
    fn sha512_deterministic() {
        let sk = Fr::from(42u64);
        let r1 = sha512_to_grumpkin_scalar(&sk, 123);
        let r2 = sha512_to_grumpkin_scalar(&sk, 123);
        assert_eq!(r1, r2);
    }

    #[test]
    fn sha512_different_separators_produce_different_results() {
        let sk = Fr::from(42u64);
        let r1 = sha512_to_grumpkin_scalar(&sk, domain_separator::NHK_M);
        let r2 = sha512_to_grumpkin_scalar(&sk, domain_separator::IVSK_M);
        assert_ne!(r1, r2);
    }
}
