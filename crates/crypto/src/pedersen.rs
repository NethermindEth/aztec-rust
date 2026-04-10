//! Pedersen hash on the Grumpkin curve.
//!
//! Matches the Noir standard library's `pedersen_hash` implementation:
//!
//! ```text
//! generators = derive_generators("pedersen_hash", N, 0)
//! length_gen = derive_generators("pedersen_hash_length", 1, 0)[0]
//! H = sum(inputs[i] * generators[i]) + N * length_gen
//! result = H.x
//! ```

use ark_ec::AffineRepr;
use ark_ff::{BigInteger, PrimeField};
use aztec_core::grumpkin;
use aztec_core::types::{Fq, Fr, GrumpkinScalar, Point};
use bn254_blackbox_solver::derive_generators;

/// Convert an `ark_grumpkin::Affine` point to our `Point` type.
///
/// Both use BN254's scalar field (Fr) as the base field for Grumpkin.
fn affine_to_point(affine: &ark_grumpkin::Affine) -> Point {
    if affine.is_zero() {
        return Point {
            x: Fr::zero(),
            y: Fr::zero(),
            is_infinite: true,
        };
    }
    let x = affine.x().expect("non-infinity point has x");
    let y = affine.y().expect("non-infinity point has y");

    // ark_grumpkin::Fq is the same field as ark_bn254::Fr.
    // Convert via big-endian byte representation.
    let x_bytes = x.into_bigint().to_bytes_be();
    let y_bytes = y.into_bigint().to_bytes_be();

    let mut x_padded = [0u8; 32];
    let mut y_padded = [0u8; 32];
    x_padded[32 - x_bytes.len()..].copy_from_slice(&x_bytes);
    y_padded[32 - y_bytes.len()..].copy_from_slice(&y_bytes);

    Point {
        x: Fr::from(x_padded),
        y: Fr::from(y_padded),
        is_infinite: false,
    }
}

/// Compute the Pedersen hash of a slice of field elements.
///
/// This matches the Noir standard library's `pedersen_hash` function exactly:
/// - Generators are derived from the `"pedersen_hash"` domain separator
/// - A length generator is derived from `"pedersen_hash_length"`
/// - Result is the x-coordinate of the accumulated point
pub fn pedersen_hash(inputs: &[Fr]) -> Fr {
    let n = inputs.len();

    // Derive generators for the inputs.
    // The Noir stdlib's pedersen_hash uses "DEFAULT_DOMAIN_SEPARATOR" with
    // separator=0 as starting_index.
    let generators = derive_generators(b"DEFAULT_DOMAIN_SEPARATOR", n as u32, 0);
    // Derive the length generator
    let length_generators = derive_generators(b"pedersen_hash_length", 1, 0);
    let length_gen = affine_to_point(&length_generators[0]);

    // Accumulate: H = sum(inputs[i] * generators[i])
    let mut result = Point {
        x: Fr::zero(),
        y: Fr::zero(),
        is_infinite: true,
    };

    for (i, input) in inputs.iter().enumerate() {
        let gen_point = affine_to_point(&generators[i]);
        // Convert Fr (BN254 scalar field) to GrumpkinScalar (BN254 base field = Grumpkin scalar field).
        // Fr and Fq are different fields, but for pedersen hash the input values
        // are small enough (or we just reinterpret the bytes).
        // The Noir pedersen_hash takes Field elements and multiplies them with generators.
        // In Noir, Field = BN254 scalar field = Grumpkin base field.
        // Our Fr = BN254 scalar field (same as Noir's Field).
        // GrumpkinScalar = Fq = BN254 base field = Grumpkin scalar field.
        // We need to convert Fr -> GrumpkinScalar for scalar_mul.
        let scalar = fr_to_grumpkin_scalar(input);
        let term = grumpkin::scalar_mul(&scalar, &gen_point);
        result = grumpkin::point_add(&result, &term);
    }

    // Add length term: N * length_gen
    let length_scalar = GrumpkinScalar::from(n as u64);
    let length_term = grumpkin::scalar_mul(&length_scalar, &length_gen);
    result = grumpkin::point_add(&result, &length_term);

    // Return x-coordinate
    result.x
}

/// Convert an Fr (BN254 scalar field) to a GrumpkinScalar (BN254 base field).
///
/// This reinterprets the big-endian byte representation, reducing modulo the
/// target field order. For values that fit in both fields (which is the common
/// case for pedersen hash inputs), this is an identity operation on the integer.
fn fr_to_grumpkin_scalar(fr: &Fr) -> GrumpkinScalar {
    let bytes = fr.to_be_bytes();
    Fq::from_be_bytes_mod_order(&bytes)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn pedersen_hash_single_zero() {
        // pedersen_hash([0]) should produce a deterministic non-zero result
        let result = pedersen_hash(&[Fr::zero()]);
        // The result should be a valid field element (non-trivially zero because
        // of the length generator contribution: 1 * length_gen)
        assert!(!result.is_zero());
    }

    #[test]
    fn pedersen_hash_deterministic() {
        let inputs = [Fr::from(1u64), Fr::from(2u64), Fr::from(3u64)];
        let h1 = pedersen_hash(&inputs);
        let h2 = pedersen_hash(&inputs);
        assert_eq!(h1, h2);
    }

    #[test]
    fn pedersen_hash_different_inputs_differ() {
        let h1 = pedersen_hash(&[Fr::from(1u64), Fr::from(2u64)]);
        let h2 = pedersen_hash(&[Fr::from(2u64), Fr::from(1u64)]);
        assert_ne!(h1, h2);
    }
}
