//! Schnorr signature scheme on the Grumpkin curve.
//!
//! Implements the same Schnorr construction used by the Noir schnorr library:
//! - Pedersen hash for binding the nonce point R to the public key
//! - Blake2s-256 for the final challenge hash
//! - Deterministic nonce via Blake2s(private_key || message)
//! - Signature = (s, e) where s and e are 32-byte scalars

use aztec_core::grumpkin;
use aztec_core::types::{Fq, Fr, GrumpkinScalar, Point};
use blake2::{Blake2s256, Digest};

use crate::pedersen::pedersen_hash;

/// A Schnorr signature (s, e) on the Grumpkin curve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchnorrSignature {
    /// The s component (32 bytes).
    pub s: [u8; 32],
    /// The e component / challenge (32 bytes).
    pub e: [u8; 32],
}

impl SchnorrSignature {
    /// Serialize to 64 bytes: `s || e`.
    pub fn to_bytes(&self) -> [u8; 64] {
        let mut out = [0u8; 64];
        out[..32].copy_from_slice(&self.s);
        out[32..].copy_from_slice(&self.e);
        out
    }

    /// Deserialize from 64 bytes: `s || e`.
    pub fn from_bytes(bytes: &[u8; 64]) -> Self {
        let mut s = [0u8; 32];
        let mut e = [0u8; 32];
        s.copy_from_slice(&bytes[..32]);
        e.copy_from_slice(&bytes[32..]);
        Self { s, e }
    }

    /// Convert signature bytes to field elements (one Fr per byte).
    ///
    /// This matches the TS SDK convention where auth witness fields
    /// are the individual signature bytes as field elements.
    pub fn to_fields(&self) -> Vec<Fr> {
        self.to_bytes()
            .iter()
            .map(|&b| Fr::from(b as u64))
            .collect()
    }
}

/// Generate a deterministic nonce for Schnorr signing.
///
/// `k = Blake2s(private_key_bytes || message_bytes)` reduced to a Grumpkin scalar.
fn generate_nonce(private_key: &GrumpkinScalar, message: &Fr) -> GrumpkinScalar {
    let mut hasher = Blake2s256::new();
    hasher.update(private_key.to_be_bytes());
    hasher.update(message.to_be_bytes());
    let hash = hasher.finalize();

    // Use 32-byte hash as a Grumpkin scalar (reduced mod order).
    // For better uniformity we'd want 64 bytes, but this matches
    // the barretenberg Schnorr nonce derivation pattern.
    GrumpkinScalar::from_be_bytes_mod_order(&hash)
}

/// Compute the Schnorr challenge matching the Noir schnorr library:
///
/// ```text
/// pedersen_h = pedersen_hash([R.x, public_key.x, public_key.y])
/// e = blake2s(pedersen_h.to_be_bytes() || message)
/// ```
fn compute_challenge(r: &Point, public_key: &Point, message: &[u8]) -> [u8; 32] {
    // Pedersen hash binds the nonce R to the signer's public key
    let pedersen_h = pedersen_hash(&[r.x, public_key.x, public_key.y]);

    let mut hasher = Blake2s256::new();
    hasher.update(pedersen_h.to_be_bytes());
    hasher.update(message);
    let hash = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&hash);
    out
}

/// Sign a message with a Grumpkin private key using Schnorr.
///
/// The signing algorithm matches the Noir schnorr library:
/// 1. `k = Blake2s(private_key || message)` (deterministic nonce)
/// 2. `R = k * G`
/// 3. `public_key = private_key * G`
/// 4. `e = Blake2s(pedersen_hash([R.x, public_key.x, public_key.y]) || message)`
/// 5. `s = k - private_key * e` (mod Grumpkin scalar order)
///
/// Returns a `SchnorrSignature` containing `(s, e)`.
pub fn schnorr_sign(private_key: &GrumpkinScalar, message: &Fr) -> SchnorrSignature {
    let g = grumpkin::generator();
    let public_key = grumpkin::scalar_mul(private_key, &g);

    // 1. Deterministic nonce
    let k = generate_nonce(private_key, message);

    // 2. R = k * G
    let r = grumpkin::scalar_mul(&k, &g);

    // 3. Challenge: e = Blake2s(pedersen_hash([R.x, pubkey.x, pubkey.y]) || message_bytes)
    let e_bytes = compute_challenge(&r, &public_key, &message.to_be_bytes());
    let e_scalar = GrumpkinScalar::from_be_bytes_mod_order(&e_bytes);

    // 4. s = k - private_key * e (mod Grumpkin scalar order)
    let s_scalar = Fq(k.0 - private_key.0 * e_scalar.0);

    SchnorrSignature {
        s: s_scalar.to_be_bytes(),
        e: e_bytes,
    }
}

/// Verify a Schnorr signature against a public key and message hash.
///
/// The verification algorithm matches the Noir schnorr library:
/// 1. `R' = s * G + e * public_key`
/// 2. `e' = Blake2s(pedersen_hash([R'.x, public_key.x, public_key.y]) || message)`
/// 3. Accept if `e == e'`
pub fn schnorr_verify(public_key: &Point, message: &Fr, signature: &SchnorrSignature) -> bool {
    let g = grumpkin::generator();

    let s_scalar = GrumpkinScalar::from_be_bytes_mod_order(&signature.s);
    let e_scalar = GrumpkinScalar::from_be_bytes_mod_order(&signature.e);

    // R' = s * G + e * public_key
    let s_g = grumpkin::scalar_mul(&s_scalar, &g);
    let e_pk = grumpkin::scalar_mul(&e_scalar, public_key);
    let r_prime = grumpkin::point_add(&s_g, &e_pk);

    // e' = Blake2s(pedersen_hash([R'.x, public_key.x, public_key.y]) || message)
    let e_prime = compute_challenge(&r_prime, public_key, &message.to_be_bytes());

    signature.e == e_prime
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::keys::{derive_public_key_from_secret_key, derive_signing_key};

    #[test]
    fn sign_and_verify_roundtrip() {
        let secret = Fr::from(12345u64);
        let signing_key = derive_signing_key(&secret);
        let public_key = derive_public_key_from_secret_key(&signing_key);

        let message = Fr::from(42u64);
        let sig = schnorr_sign(&signing_key, &message);

        assert!(schnorr_verify(&public_key, &message, &sig));
    }

    #[test]
    fn wrong_message_fails_verification() {
        let secret = Fr::from(12345u64);
        let signing_key = derive_signing_key(&secret);
        let public_key = derive_public_key_from_secret_key(&signing_key);

        let message = Fr::from(42u64);
        let sig = schnorr_sign(&signing_key, &message);

        let wrong_message = Fr::from(43u64);
        assert!(!schnorr_verify(&public_key, &wrong_message, &sig));
    }

    #[test]
    fn wrong_key_fails_verification() {
        let secret = Fr::from(12345u64);
        let signing_key = derive_signing_key(&secret);

        let other_secret = Fr::from(99999u64);
        let other_signing_key = derive_signing_key(&other_secret);
        let other_public_key = derive_public_key_from_secret_key(&other_signing_key);

        let message = Fr::from(42u64);
        let sig = schnorr_sign(&signing_key, &message);

        assert!(!schnorr_verify(&other_public_key, &message, &sig));
    }

    #[test]
    fn signing_is_deterministic() {
        let secret = Fr::from(8923u64);
        let signing_key = derive_signing_key(&secret);

        let message = Fr::from(1000u64);
        let sig1 = schnorr_sign(&signing_key, &message);
        let sig2 = schnorr_sign(&signing_key, &message);

        assert_eq!(sig1, sig2);
    }

    #[test]
    fn different_messages_produce_different_signatures() {
        let secret = Fr::from(8923u64);
        let signing_key = derive_signing_key(&secret);

        let sig1 = schnorr_sign(&signing_key, &Fr::from(1u64));
        let sig2 = schnorr_sign(&signing_key, &Fr::from(2u64));

        assert_ne!(sig1, sig2);
    }

    #[test]
    fn signature_serialization_roundtrip() {
        let secret = Fr::from(42u64);
        let signing_key = derive_signing_key(&secret);

        let sig = schnorr_sign(&signing_key, &Fr::from(100u64));
        let bytes = sig.to_bytes();
        let recovered = SchnorrSignature::from_bytes(&bytes);

        assert_eq!(sig, recovered);
    }

    #[test]
    fn to_fields_produces_64_elements() {
        let secret = Fr::from(42u64);
        let signing_key = derive_signing_key(&secret);

        let sig = schnorr_sign(&signing_key, &Fr::from(100u64));
        let fields = sig.to_fields();

        assert_eq!(fields.len(), 64);
        // Each field should be a byte value (0..256)
        for field in &fields {
            let val = field.to_usize();
            assert!(val < 256);
        }
    }

    #[test]
    fn verify_with_realistic_key_derivation() {
        // Simulate the full flow: secret_key -> signing_key -> public_key -> sign -> verify
        let secret_key = Fr::from(8923u64);
        let signing_key = derive_signing_key(&secret_key);
        let public_key = derive_public_key_from_secret_key(&signing_key);

        // Sign a realistic-looking message hash
        let message_hash =
            Fr::from_hex("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef")
                .expect("valid hex");
        let sig = schnorr_sign(&signing_key, &message_hash);

        assert!(schnorr_verify(&public_key, &message_hash, &sig));
    }
}
