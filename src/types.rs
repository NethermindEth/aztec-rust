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

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fr(pub ArkFr);

impl Fr {
    pub const fn zero() -> Self {
        Self(ArkFr::ZERO)
    }

    pub const fn one() -> Self {
        Self(ArkFr::ONE)
    }

    pub fn from_hex(value: &str) -> Result<Self, Error> {
        let bytes = decode_fixed_hex::<32>(value)?;
        Ok(Self(ArkFr::from_be_bytes_mod_order(&bytes)))
    }

    pub fn random() -> Self {
        Self(ArkFr::rand(&mut rand::thread_rng()))
    }
}

impl From<u64> for Fr {
    fn from(value: u64) -> Self {
        Self(ArkFr::from(value))
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

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fq(pub ArkFq);

impl Fq {
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

pub type GrumpkinScalar = Fr;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Point {
    pub x: Fr,
    pub y: Fr,
    pub is_infinite: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct AztecAddress(pub Fr);

impl From<Fr> for AztecAddress {
    fn from(value: Fr) -> Self {
        Self(value)
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicKeys {
    pub master_nullifier_public_key: Point,
    pub master_incoming_viewing_public_key: Point,
    pub master_outgoing_viewing_public_key: Point,
    pub master_tagging_public_key: Point,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompleteAddress {
    pub address: AztecAddress,
    pub public_keys: PublicKeys,
    pub partial_address: Fr,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractInstance {
    pub version: u8,
    pub salt: Fr,
    pub deployer: AztecAddress,
    pub current_contract_class_id: Fr,
    pub original_contract_class_id: Fr,
    pub initialization_hash: Fr,
    pub public_keys: PublicKeys,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractInstanceWithAddress {
    pub address: AztecAddress,
    #[serde(flatten)]
    pub inner: ContractInstance,
}

impl std::ops::Deref for ContractInstanceWithAddress {
    type Target = ContractInstance;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

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
}
