use ark_bn254::{Fq as ArkFq, Fr as ArkFr};
use ark_ff::{AdditiveGroup, BigInteger, Field, PrimeField, UniformRand};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

use crate::Error;

pub(crate) fn strip_0x(s: &str) -> &str {
    s.strip_prefix("0x").unwrap_or(s)
}

pub(crate) fn decode_fixed_hex<const N: usize>(s: &str) -> Result<[u8; N], Error> {
    let raw = strip_0x(s);
    if raw.len() > N * 2 {
        return Err(Error::InvalidData(format!(
            "hex value too large: expected at most {N} bytes",
        )));
    }

    let padded = if raw.len() % 2 == 1 {
        format!("0{raw}")
    } else {
        raw.to_owned()
    };

    let decoded = hex::decode(padded).map_err(|e| Error::InvalidData(e.to_string()))?;
    if decoded.len() > N {
        return Err(Error::InvalidData(format!(
            "hex value too large: expected at most {N} bytes",
        )));
    }

    let mut out = [0u8; N];
    out[N - decoded.len()..].copy_from_slice(&decoded);
    Ok(out)
}

pub(crate) fn encode_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

fn encode_field_hex<F: PrimeField>(value: &F) -> String {
    let raw = value.into_bigint().to_bytes_be();
    let mut padded = [0u8; 32];
    padded[32 - raw.len()..].copy_from_slice(&raw);
    encode_hex(&padded)
}

/// A BN254 scalar field element.
///
/// This is the main field type used throughout the Aztec protocol for
/// addresses, hashes, note values, and other scalar quantities.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fr(pub ArkFr);

impl Fr {
    /// The additive identity (zero).
    pub const fn zero() -> Self {
        Self(ArkFr::ZERO)
    }

    /// The multiplicative identity (one).
    pub const fn one() -> Self {
        Self(ArkFr::ONE)
    }

    /// Parse from a hex string (e.g. `"0x01"`).
    pub fn from_hex(value: &str) -> Result<Self, Error> {
        let bytes = decode_fixed_hex::<32>(value)?;
        Ok(Self(ArkFr::from_be_bytes_mod_order(&bytes)))
    }

    /// Generate a random field element.
    pub fn random() -> Self {
        Self(ArkFr::rand(&mut rand::thread_rng()))
    }

    /// Serialize to 32-byte big-endian representation.
    pub fn to_be_bytes(&self) -> [u8; 32] {
        let raw = self.0.into_bigint().to_bytes_be();
        let mut out = [0u8; 32];
        out[32 - raw.len()..].copy_from_slice(&raw);
        out
    }

    /// Extract as a `usize`. Only valid for small field elements.
    ///
    /// Panics on overflow if the field element exceeds `usize::MAX`.
    pub fn to_usize(&self) -> usize {
        let raw = self.0.into_bigint().to_bytes_be();
        raw.into_iter()
            .fold(0usize, |acc, byte| (acc << 8) | usize::from(byte))
    }

    /// Returns `true` if this field element is zero.
    pub fn is_zero(&self) -> bool {
        *self == Self::zero()
    }
}

impl From<u64> for Fr {
    fn from(value: u64) -> Self {
        Self(ArkFr::from(value))
    }
}

impl From<i64> for Fr {
    fn from(value: i64) -> Self {
        if value >= 0 {
            Self(ArkFr::from(value as u64))
        } else {
            Self(-ArkFr::from(value.unsigned_abs()))
        }
    }
}

impl From<u128> for Fr {
    fn from(value: u128) -> Self {
        let bytes = value.to_be_bytes();
        let mut padded = [0u8; 32];
        padded[16..].copy_from_slice(&bytes);
        Self(ArkFr::from_be_bytes_mod_order(&padded))
    }
}

impl From<bool> for Fr {
    fn from(value: bool) -> Self {
        if value {
            Self::one()
        } else {
            Self::zero()
        }
    }
}

impl From<[u8; 32]> for Fr {
    fn from(value: [u8; 32]) -> Self {
        Self(ArkFr::from_be_bytes_mod_order(&value))
    }
}

impl fmt::Display for Fr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&encode_field_hex(&self.0))
    }
}

impl fmt::Debug for Fr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fr({self})")
    }
}

impl Serialize for Fr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Fr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

/// A BN254 base field element.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fq(pub ArkFq);

impl Fq {
    /// Parse from a hex string.
    pub fn from_hex(value: &str) -> Result<Self, Error> {
        let bytes = decode_fixed_hex::<32>(value)?;
        Ok(Self(ArkFq::from_be_bytes_mod_order(&bytes)))
    }
}

impl fmt::Display for Fq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&encode_field_hex(&self.0))
    }
}

impl fmt::Debug for Fq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fq({self})")
    }
}

impl Serialize for Fq {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Fq {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

/// Type alias for Grumpkin curve scalars (same as [`Fr`]).
pub type GrumpkinScalar = Fr;

/// A point on the Grumpkin curve, used for Aztec public keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Point {
    /// X coordinate.
    pub x: Fr,
    /// Y coordinate.
    pub y: Fr,
    /// Whether this is the point at infinity.
    pub is_infinite: bool,
}

/// An Aztec L2 address, represented as a single field element.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct AztecAddress(pub Fr);

impl From<Fr> for AztecAddress {
    fn from(value: Fr) -> Self {
        Self(value)
    }
}

impl From<u64> for AztecAddress {
    fn from(value: u64) -> Self {
        Self(Fr::from(value))
    }
}

impl From<AztecAddress> for Fr {
    fn from(value: AztecAddress) -> Self {
        value.0
    }
}

impl From<EthAddress> for Fr {
    fn from(value: EthAddress) -> Self {
        let mut padded = [0u8; 32];
        padded[12..].copy_from_slice(&value.0);
        Self(ArkFr::from_be_bytes_mod_order(&padded))
    }
}

impl From<crate::abi::FunctionSelector> for Fr {
    fn from(value: crate::abi::FunctionSelector) -> Self {
        value.to_field()
    }
}

impl fmt::Display for AztecAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Debug for AztecAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AztecAddress({})", self.0)
    }
}

impl Serialize for AztecAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AztecAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self(Fr::deserialize(deserializer)?))
    }
}

/// An Ethereum L1 address (20 bytes).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EthAddress(pub [u8; 20]);

impl From<[u8; 20]> for EthAddress {
    fn from(value: [u8; 20]) -> Self {
        Self(value)
    }
}

impl fmt::Display for EthAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&encode_hex(&self.0))
    }
}

impl fmt::Debug for EthAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EthAddress({self})")
    }
}

impl Serialize for EthAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for EthAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = decode_fixed_hex::<20>(&s).map_err(serde::de::Error::custom)?;
        Ok(Self(bytes))
    }
}

/// The set of master public keys associated with an Aztec account.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicKeys {
    /// Key used for nullifier derivation.
    pub master_nullifier_public_key: Point,
    /// Key used to encrypt incoming notes.
    pub master_incoming_viewing_public_key: Point,
    /// Key used to encrypt outgoing note logs.
    pub master_outgoing_viewing_public_key: Point,
    /// Key used for note tagging.
    pub master_tagging_public_key: Point,
}

impl Point {
    /// Returns `true` if this is the zero/default point (all fields zero).
    pub fn is_zero(&self) -> bool {
        self.x == Fr::zero() && self.y == Fr::zero() && !self.is_infinite
    }
}

impl PublicKeys {
    /// Returns `true` if all keys are zero (default/empty public keys).
    pub fn is_empty(&self) -> bool {
        self.master_nullifier_public_key.is_zero()
            && self.master_incoming_viewing_public_key.is_zero()
            && self.master_outgoing_viewing_public_key.is_zero()
            && self.master_tagging_public_key.is_zero()
    }

    /// Compute the public keys hash.
    ///
    /// Returns `Fr::zero()` if all keys are empty, matching the upstream
    /// TS behavior in `public_keys.ts`.
    ///
    /// Otherwise hashes the four key points with the `PUBLIC_KEYS_HASH`
    /// domain separator using Poseidon2.
    pub fn hash(&self) -> Fr {
        if self.is_empty() {
            return Fr::zero();
        }

        use crate::constants::domain_separator;
        use crate::hash::poseidon2_hash_with_separator;

        // Flatten each point to [x, y, is_infinite] and hash.
        let fields = [
            self.master_nullifier_public_key.x,
            self.master_nullifier_public_key.y,
            if self.master_nullifier_public_key.is_infinite {
                Fr::one()
            } else {
                Fr::zero()
            },
            self.master_incoming_viewing_public_key.x,
            self.master_incoming_viewing_public_key.y,
            if self.master_incoming_viewing_public_key.is_infinite {
                Fr::one()
            } else {
                Fr::zero()
            },
            self.master_outgoing_viewing_public_key.x,
            self.master_outgoing_viewing_public_key.y,
            if self.master_outgoing_viewing_public_key.is_infinite {
                Fr::one()
            } else {
                Fr::zero()
            },
            self.master_tagging_public_key.x,
            self.master_tagging_public_key.y,
            if self.master_tagging_public_key.is_infinite {
                Fr::one()
            } else {
                Fr::zero()
            },
        ];

        poseidon2_hash_with_separator(&fields, domain_separator::PUBLIC_KEYS_HASH)
    }
}

/// A complete address combining the Aztec address, public keys, and partial address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompleteAddress {
    /// The Aztec L2 address.
    pub address: AztecAddress,
    /// The account's public keys.
    pub public_keys: PublicKeys,
    /// The partial address (used in address derivation).
    pub partial_address: Fr,
}

/// An Aztec contract instance (without address).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractInstance {
    /// Instance version.
    pub version: u8,
    /// Deployment salt for address derivation.
    pub salt: Fr,
    /// Address of the deployer.
    pub deployer: AztecAddress,
    /// Current contract class ID (may differ from original after upgrades).
    pub current_contract_class_id: Fr,
    /// Original contract class ID at deployment time.
    pub original_contract_class_id: Fr,
    /// Hash of the initialization arguments.
    pub initialization_hash: Fr,
    /// Public keys associated with this instance.
    pub public_keys: PublicKeys,
}

/// An Aztec contract instance with its derived address.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractInstanceWithAddress {
    /// The contract's derived address.
    pub address: AztecAddress,
    /// The contract instance data.
    #[serde(flatten)]
    pub inner: ContractInstance,
}

impl std::ops::Deref for ContractInstanceWithAddress {
    type Target = ContractInstance;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// Any value that can be converted into a field element.
/// This is the Rust equivalent of the TS `FieldLike` type.
pub type FieldLike = Fr;

/// Any value that can be converted into an Aztec L2 address.
pub type AztecAddressLike = AztecAddress;

/// Any value that can be converted into an Ethereum L1 address.
pub type EthAddressLike = EthAddress;

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn fr_json_roundtrip() {
        let value = Fr::from(1u64);
        let json = match serde_json::to_string(&value) {
            Ok(json) => json,
            Err(err) => panic!("serializing Fr should succeed: {err}"),
        };
        assert_eq!(
            json,
            "\"0x0000000000000000000000000000000000000000000000000000000000000001\""
        );

        let decoded: Fr = match serde_json::from_str(&json) {
            Ok(decoded) => decoded,
            Err(err) => panic!("deserializing Fr should succeed: {err}"),
        };
        assert_eq!(decoded, value);
    }

    #[test]
    fn point_json_roundtrip() {
        let point = Point {
            x: Fr::from(1u64),
            y: Fr::from(2u64),
            is_infinite: false,
        };

        let json = match serde_json::to_string(&point) {
            Ok(json) => json,
            Err(err) => panic!("serializing Point should succeed: {err}"),
        };
        let decoded: Point = match serde_json::from_str(&json) {
            Ok(decoded) => decoded,
            Err(err) => panic!("deserializing Point should succeed: {err}"),
        };
        assert_eq!(decoded, point);
    }

    #[test]
    fn fr_helpers_work() {
        assert_eq!(
            Fr::zero().to_string(),
            "0x0000000000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(
            Fr::one().to_string(),
            "0x0000000000000000000000000000000000000000000000000000000000000001"
        );
    }

    #[test]
    fn fr_from_hex_rejects_invalid() {
        let Err(err) = Fr::from_hex("not-hex") else {
            panic!("invalid hex should be rejected");
        };
        assert!(matches!(err, Error::InvalidData(_)));
    }

    #[test]
    fn aztec_address_roundtrip() {
        let address = AztecAddress(Fr::from(7u64));
        let json = match serde_json::to_string(&address) {
            Ok(json) => json,
            Err(err) => panic!("serializing AztecAddress should succeed: {err}"),
        };
        let decoded: AztecAddress = match serde_json::from_str(&json) {
            Ok(decoded) => decoded,
            Err(err) => panic!("deserializing AztecAddress should succeed: {err}"),
        };
        assert_eq!(decoded, address);
    }

    #[test]
    fn eth_address_roundtrip() {
        let address = EthAddress([0x11; 20]);
        let json = match serde_json::to_string(&address) {
            Ok(json) => json,
            Err(err) => panic!("serializing EthAddress should succeed: {err}"),
        };
        let decoded: EthAddress = match serde_json::from_str(&json) {
            Ok(decoded) => decoded,
            Err(err) => panic!("deserializing EthAddress should succeed: {err}"),
        };
        assert_eq!(decoded, address);
    }

    #[test]
    fn public_keys_and_complete_address_roundtrip() {
        let keys = PublicKeys {
            master_nullifier_public_key: Point {
                x: Fr::from(1u64),
                y: Fr::from(2u64),
                is_infinite: false,
            },
            master_incoming_viewing_public_key: Point {
                x: Fr::from(3u64),
                y: Fr::from(4u64),
                is_infinite: false,
            },
            master_outgoing_viewing_public_key: Point {
                x: Fr::from(5u64),
                y: Fr::from(6u64),
                is_infinite: false,
            },
            master_tagging_public_key: Point {
                x: Fr::from(7u64),
                y: Fr::from(8u64),
                is_infinite: false,
            },
        };
        let complete = CompleteAddress {
            address: AztecAddress(Fr::from(9u64)),
            public_keys: keys,
            partial_address: Fr::from(10u64),
        };

        let json = match serde_json::to_string(&complete) {
            Ok(json) => json,
            Err(err) => panic!("serializing CompleteAddress should succeed: {err}"),
        };
        let decoded: CompleteAddress = match serde_json::from_str(&json) {
            Ok(decoded) => decoded,
            Err(err) => panic!("deserializing CompleteAddress should succeed: {err}"),
        };
        assert_eq!(decoded, complete);
    }

    #[test]
    fn contract_instance_roundtrip() {
        let instance = ContractInstance {
            version: 1,
            salt: Fr::from(1u64),
            deployer: AztecAddress(Fr::from(2u64)),
            current_contract_class_id: Fr::from(3u64),
            original_contract_class_id: Fr::from(4u64),
            initialization_hash: Fr::from(5u64),
            public_keys: PublicKeys::default(),
        };

        let wrapped = ContractInstanceWithAddress {
            address: AztecAddress(Fr::from(6u64)),
            inner: instance,
        };

        let json = match serde_json::to_string(&wrapped) {
            Ok(json) => json,
            Err(err) => panic!("serializing ContractInstanceWithAddress should succeed: {err}"),
        };
        let decoded: ContractInstanceWithAddress = match serde_json::from_str(&json) {
            Ok(decoded) => decoded,
            Err(err) => panic!("deserializing ContractInstanceWithAddress should succeed: {err}"),
        };
        assert_eq!(decoded, wrapped);
        assert_eq!(decoded.version, 1);
    }

    // -- Substep 6.0: Fr helper methods --

    #[test]
    fn fr_to_be_bytes_zero() {
        assert_eq!(Fr::zero().to_be_bytes(), [0u8; 32]);
    }

    #[test]
    fn fr_to_be_bytes_one() {
        let bytes = Fr::from(1u64).to_be_bytes();
        assert_eq!(bytes[31], 0x01);
        assert_eq!(bytes[..31], [0u8; 31]);
    }

    #[test]
    fn fr_to_usize_roundtrip() {
        assert_eq!(Fr::from(42u64).to_usize(), 42);
        assert_eq!(Fr::from(0u64).to_usize(), 0);
        assert_eq!(Fr::from(255u64).to_usize(), 255);
    }

    #[test]
    fn fr_is_zero() {
        assert!(Fr::zero().is_zero());
        assert!(!Fr::one().is_zero());
        assert!(!Fr::from(42u64).is_zero());
    }

    // -- Substep 6.6: From impls --

    #[test]
    fn fr_from_bool() {
        assert_eq!(Fr::from(true), Fr::one());
        assert_eq!(Fr::from(false), Fr::zero());
    }

    #[test]
    fn fr_from_i64_positive() {
        assert_eq!(Fr::from(42i64), Fr::from(42u64));
        assert_eq!(Fr::from(0i64), Fr::zero());
    }

    #[test]
    fn fr_from_i64_negative() {
        // -1 in the field should satisfy: Fr::from(-1i64) + Fr::one() == Fr::zero()
        let neg_one = Fr::from(-1i64);
        let sum = Fr(neg_one.0 + Fr::one().0);
        assert_eq!(sum, Fr::zero());
    }

    #[test]
    fn fr_from_u128() {
        let big: u128 = (1u128 << 64) + 42;
        let fr = Fr::from(big);
        // Should not be zero (it's a big number)
        assert!(!fr.is_zero());
        // Small u128 should match u64
        assert_eq!(Fr::from(100u128), Fr::from(100u64));
    }

    #[test]
    fn fr_from_aztec_address() {
        let addr = AztecAddress(Fr::from(7u64));
        assert_eq!(Fr::from(addr), Fr::from(7u64));
    }

    #[test]
    fn fr_from_eth_address() {
        let eth = EthAddress([0x11; 20]);
        let fr = Fr::from(eth);
        // Should produce a field with the 20 bytes right-aligned in 32 bytes
        let bytes = fr.to_be_bytes();
        assert_eq!(&bytes[12..], &[0x11; 20]);
        assert_eq!(&bytes[..12], &[0u8; 12]);
    }

    #[test]
    fn fr_from_function_selector() {
        let selector = crate::abi::FunctionSelector::from_hex("0xaabbccdd").expect("valid hex");
        assert_eq!(Fr::from(selector), selector.to_field());
    }

    #[test]
    fn aztec_address_from_u64() {
        let addr = AztecAddress::from(42u64);
        assert_eq!(addr.0, Fr::from(42u64));
    }
}
