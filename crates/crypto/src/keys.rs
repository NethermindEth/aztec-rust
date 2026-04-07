//! Key derivation for the Aztec protocol.
//!
//! Implements the full key hierarchy: master secret keys via SHA-512,
//! master public keys via Grumpkin scalar multiplication, and
//! app-scoped keys via Poseidon2.

use aztec_core::constants::domain_separator;
use aztec_core::grumpkin;
use aztec_core::hash::poseidon2_hash_with_separator;
use aztec_core::types::{AztecAddress, Fr, GrumpkinScalar, Point, PublicKeys};

use crate::sha512::sha512_to_grumpkin_scalar;

// ---------------------------------------------------------------------------
// Master key derivation (Sub-step 7.3)
// ---------------------------------------------------------------------------

/// Derive the master nullifier hiding key from a secret key.
pub fn derive_master_nullifier_hiding_key(secret_key: &Fr) -> GrumpkinScalar {
    sha512_to_grumpkin_scalar(secret_key, domain_separator::NHK_M)
}

/// Derive the master incoming viewing secret key from a secret key.
pub fn derive_master_incoming_viewing_secret_key(secret_key: &Fr) -> GrumpkinScalar {
    sha512_to_grumpkin_scalar(secret_key, domain_separator::IVSK_M)
}

/// Derive the master outgoing viewing secret key from a secret key.
pub fn derive_master_outgoing_viewing_secret_key(secret_key: &Fr) -> GrumpkinScalar {
    sha512_to_grumpkin_scalar(secret_key, domain_separator::OVSK_M)
}

/// Derive the master tagging secret key from a secret key.
pub fn derive_master_tagging_secret_key(secret_key: &Fr) -> GrumpkinScalar {
    sha512_to_grumpkin_scalar(secret_key, domain_separator::TSK_M)
}

/// Derive the signing key from a secret key.
///
/// Currently uses the same derivation as IVSK_M (see TS TODO #5837).
pub fn derive_signing_key(secret_key: &Fr) -> GrumpkinScalar {
    sha512_to_grumpkin_scalar(secret_key, domain_separator::IVSK_M)
}

// ---------------------------------------------------------------------------
// Public key derivation (Sub-step 7.4)
// ---------------------------------------------------------------------------

/// Derive a Grumpkin public key from a secret key via scalar multiplication.
///
/// `public_key = secret_key * G`
pub fn derive_public_key_from_secret_key(secret_key: &GrumpkinScalar) -> Point {
    let g = grumpkin::generator();
    grumpkin::scalar_mul(secret_key, &g)
}

// ---------------------------------------------------------------------------
// Full key set derivation (Sub-step 7.5)
// ---------------------------------------------------------------------------

/// The complete set of derived keys from a secret key.
pub struct DerivedKeys {
    /// Master nullifier hiding key (secret).
    pub master_nullifier_hiding_key: GrumpkinScalar,
    /// Master incoming viewing secret key.
    pub master_incoming_viewing_secret_key: GrumpkinScalar,
    /// Master outgoing viewing secret key.
    pub master_outgoing_viewing_secret_key: GrumpkinScalar,
    /// Master tagging secret key.
    pub master_tagging_secret_key: GrumpkinScalar,
    /// The four master public keys.
    pub public_keys: PublicKeys,
}

/// Derive the complete key set from a secret key.
pub fn derive_keys(secret_key: &Fr) -> DerivedKeys {
    // 1. Derive master secret keys via SHA-512
    let nhk_m = derive_master_nullifier_hiding_key(secret_key);
    let ivsk_m = derive_master_incoming_viewing_secret_key(secret_key);
    let ovsk_m = derive_master_outgoing_viewing_secret_key(secret_key);
    let tsk_m = derive_master_tagging_secret_key(secret_key);

    // 2. Derive master public keys via Grumpkin scalar multiplication
    let npk_m = derive_public_key_from_secret_key(&nhk_m);
    let ivpk_m = derive_public_key_from_secret_key(&ivsk_m);
    let ovpk_m = derive_public_key_from_secret_key(&ovsk_m);
    let tpk_m = derive_public_key_from_secret_key(&tsk_m);

    DerivedKeys {
        master_nullifier_hiding_key: nhk_m,
        master_incoming_viewing_secret_key: ivsk_m,
        master_outgoing_viewing_secret_key: ovsk_m,
        master_tagging_secret_key: tsk_m,
        public_keys: PublicKeys {
            master_nullifier_public_key: npk_m,
            master_incoming_viewing_public_key: ivpk_m,
            master_outgoing_viewing_public_key: ovpk_m,
            master_tagging_public_key: tpk_m,
        },
    }
}

// ---------------------------------------------------------------------------
// App-scoped key derivation (Sub-step 7.6)
// ---------------------------------------------------------------------------

/// Identifies a key type for app-scoped derivation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyType {
    Nullifier,
    IncomingViewing,
    OutgoingViewing,
    Tagging,
}

impl KeyType {
    pub fn domain_separator(&self) -> u32 {
        match self {
            KeyType::Nullifier => domain_separator::NHK_M,
            KeyType::IncomingViewing => domain_separator::IVSK_M,
            KeyType::OutgoingViewing => domain_separator::OVSK_M,
            KeyType::Tagging => domain_separator::TSK_M,
        }
    }
}

/// Compute an app-scoped secret key from a master key and app address.
///
/// `app_key = poseidon2([master_key.hi, master_key.lo, app], domain_separator)`
pub fn compute_app_secret_key(
    master_key: &GrumpkinScalar,
    app: &AztecAddress,
    key_type: KeyType,
) -> Fr {
    let hi = master_key.hi();
    let lo = master_key.lo();
    let separator = key_type.domain_separator();
    poseidon2_hash_with_separator(&[hi, lo, Fr::from(*app)], separator)
}

/// Compute the app-scoped nullifier hiding key.
pub fn compute_app_nullifier_hiding_key(
    master_nullifier_hiding_key: &GrumpkinScalar,
    app: &AztecAddress,
) -> Fr {
    compute_app_secret_key(master_nullifier_hiding_key, app, KeyType::Nullifier)
}

/// Compute the app-scoped outgoing viewing secret key.
///
/// Returns `GrumpkinScalar` (Fq) because the result is used for scalar
/// multiplication when encrypting outgoing note logs.
pub fn compute_ovsk_app(
    master_outgoing_viewing_key: &GrumpkinScalar,
    app: &AztecAddress,
) -> GrumpkinScalar {
    let fr_result =
        compute_app_secret_key(master_outgoing_viewing_key, app, KeyType::OutgoingViewing);
    // Intentional Fr -> Fq conversion. Distribution is not perfectly uniform
    // but 2*(q-r)/q is negligibly small.
    GrumpkinScalar::from_be_bytes_mod_order(&fr_result.to_be_bytes())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    // TS test vectors from key_store.test.ts with secret_key = 8923n

    #[test]
    fn master_key_derivation_nhk_m() {
        let sk = Fr::from(8923u64);
        let nhk_m = derive_master_nullifier_hiding_key(&sk);
        let expected = GrumpkinScalar::from_hex(
            "0x26dd6f83a99b5b1cea47692f40b7aece47756a1a5e93138c5b8f7e7afd36ed1a",
        )
        .expect("valid hex");
        assert_eq!(nhk_m, expected);
    }

    #[test]
    fn master_key_derivation_ivsk_m() {
        let sk = Fr::from(8923u64);
        let ivsk_m = derive_master_incoming_viewing_secret_key(&sk);
        let expected = GrumpkinScalar::from_hex(
            "0x0d3e4402946f2f712d942e1a3962b12fc521effc39fe93777f91285f1ad414cb",
        )
        .expect("valid hex");
        assert_eq!(ivsk_m, expected);
    }

    #[test]
    fn public_key_derivation_npk_m() {
        let sk = Fr::from(8923u64);
        let nhk_m = derive_master_nullifier_hiding_key(&sk);
        let npk_m = derive_public_key_from_secret_key(&nhk_m);
        let expected_x =
            Fr::from_hex("0x0d86b380f66ec74d32bb04d98f5b2dcef6d92f344e65604a21640f87fb6d078e")
                .expect("valid hex");
        let expected_y =
            Fr::from_hex("0x2b68df4d20985b71c252746a3f2cc5af32b5f0c32739b94f166dfa230f50397b")
                .expect("valid hex");
        assert_eq!(npk_m.x, expected_x);
        assert_eq!(npk_m.y, expected_y);
        assert!(!npk_m.is_infinite);
    }

    #[test]
    fn public_key_derivation_ivpk_m() {
        let sk = Fr::from(8923u64);
        let ivsk_m = derive_master_incoming_viewing_secret_key(&sk);
        let ivpk_m = derive_public_key_from_secret_key(&ivsk_m);
        let expected_x =
            Fr::from_hex("0x0e0eb5bc3eb9959d6e05cbc0e37b2fa4cfb113c1db651c384907547f1f867010")
                .expect("valid hex");
        let expected_y =
            Fr::from_hex("0x1db2e49c6845619ba432a951d86de2d41680157b0f54556246916900c0fcdcf2")
                .expect("valid hex");
        assert_eq!(ivpk_m.x, expected_x);
        assert_eq!(ivpk_m.y, expected_y);
    }

    #[test]
    fn public_key_derivation_ovpk_m() {
        let sk = Fr::from(8923u64);
        let ovsk_m = derive_master_outgoing_viewing_secret_key(&sk);
        let ovpk_m = derive_public_key_from_secret_key(&ovsk_m);
        let expected_x =
            Fr::from_hex("0x2721eaed30c0c9fae14c2ca4af7668a46278762d4a6066ab7a5defcc242f559c")
                .expect("valid hex");
        let expected_y =
            Fr::from_hex("0x0bd0c4b0ec90ebafe511f20e818fb359a1322ab0f02fe3ebec95af5df502015d")
                .expect("valid hex");
        assert_eq!(ovpk_m.x, expected_x);
        assert_eq!(ovpk_m.y, expected_y);
    }

    #[test]
    fn public_key_derivation_tpk_m() {
        let sk = Fr::from(8923u64);
        let tsk_m = derive_master_tagging_secret_key(&sk);
        let tpk_m = derive_public_key_from_secret_key(&tsk_m);
        let expected_x =
            Fr::from_hex("0x0fabb6adca7c2bf7f6202c65fe2785096efb317897bc545c427635a61d536955")
                .expect("valid hex");
        let expected_y =
            Fr::from_hex("0x2cc356e6e5b68fd64d33c96fad7bb1394956c53930fefdf0bb536812ec604459")
                .expect("valid hex");
        assert_eq!(tpk_m.x, expected_x);
        assert_eq!(tpk_m.y, expected_y);
    }

    #[test]
    fn derive_keys_full_round_trip() {
        let sk = Fr::from(8923u64);
        let derived = derive_keys(&sk);

        // Verify nhk_m
        let expected_nhk_m = GrumpkinScalar::from_hex(
            "0x26dd6f83a99b5b1cea47692f40b7aece47756a1a5e93138c5b8f7e7afd36ed1a",
        )
        .expect("valid hex");
        assert_eq!(derived.master_nullifier_hiding_key, expected_nhk_m);

        // Verify npk_m
        let expected_npk_x =
            Fr::from_hex("0x0d86b380f66ec74d32bb04d98f5b2dcef6d92f344e65604a21640f87fb6d078e")
                .expect("valid hex");
        assert_eq!(
            derived.public_keys.master_nullifier_public_key.x,
            expected_npk_x
        );

        // Verify ivpk_m
        let expected_ivpk_x =
            Fr::from_hex("0x0e0eb5bc3eb9959d6e05cbc0e37b2fa4cfb113c1db651c384907547f1f867010")
                .expect("valid hex");
        assert_eq!(
            derived.public_keys.master_incoming_viewing_public_key.x,
            expected_ivpk_x
        );

        // Verify public_keys_hash is non-zero
        let pk_hash = derived.public_keys.hash();
        assert!(!pk_hash.is_zero());
    }

    #[test]
    fn app_nullifier_hiding_key() {
        let sk = Fr::from(8923u64);
        let nhk_m = derive_master_nullifier_hiding_key(&sk);
        let app = AztecAddress::from(624u64);
        let app_nhk = compute_app_nullifier_hiding_key(&nhk_m, &app);
        let expected =
            Fr::from_hex("0x165cc265d187ed42f0e3f5adbb5a0055a77e205daeb68dd1735796ee402e502f")
                .expect("valid hex");
        assert_eq!(app_nhk, expected);
    }

    #[test]
    fn app_ovsk() {
        let sk = Fr::from(8923u64);
        let ovsk_m = derive_master_outgoing_viewing_secret_key(&sk);
        let app = AztecAddress::from(624u64);
        let ovsk_app = compute_ovsk_app(&ovsk_m, &app);
        let expected = GrumpkinScalar::from_hex(
            "0x058452c94b1d8540a39d9343758fc132af3401237bd1ac2a16c37462a173954a",
        )
        .expect("valid hex");
        assert_eq!(ovsk_app, expected);
    }

    #[test]
    fn signing_key_equals_ivsk() {
        let sk = Fr::from(8923u64);
        let ivsk = derive_master_incoming_viewing_secret_key(&sk);
        let signing = derive_signing_key(&sk);
        assert_eq!(ivsk, signing);
    }

    #[test]
    fn fq_hi_lo_split() {
        let sk = Fr::from(8923u64);
        let nhk_m = derive_master_nullifier_hiding_key(&sk);

        let hi = nhk_m.hi();
        let lo = nhk_m.lo();

        // Both should be non-zero for a typical key
        assert!(!hi.is_zero());
        assert!(!lo.is_zero());

        // Reconstruct: (hi << 128) | lo should equal original (within Fq bounds)
        let hi_bytes = hi.to_be_bytes();
        let lo_bytes = lo.to_be_bytes();
        let mut reconstructed = [0u8; 32];
        // hi occupies bytes [0..16], lo occupies bytes [16..32]
        reconstructed[..16].copy_from_slice(&hi_bytes[16..]);
        reconstructed[16..].copy_from_slice(&lo_bytes[16..]);
        let original_bytes = nhk_m.to_be_bytes();
        assert_eq!(reconstructed, original_bytes);
    }
}
