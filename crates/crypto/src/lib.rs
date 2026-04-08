//! Cryptographic primitives and key derivation for the Aztec protocol.

mod sha512;

pub mod address;
pub mod keys;
pub mod schnorr;

pub use address::complete_address_from_secret_key_and_partial_address;
pub use keys::{
    compute_app_nullifier_hiding_key, compute_app_secret_key, compute_ovsk_app, derive_keys,
    derive_master_incoming_viewing_secret_key, derive_master_nullifier_hiding_key,
    derive_master_outgoing_viewing_secret_key, derive_master_tagging_secret_key,
    derive_public_key_from_secret_key, derive_signing_key, DerivedKeys, KeyType,
};
pub use schnorr::{schnorr_sign, schnorr_verify, SchnorrSignature};
pub use sha512::sha512_to_grumpkin_scalar;

// Re-export from core for API ergonomics
pub use aztec_core::hash::{compute_address, compute_secret_hash};
