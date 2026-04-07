use ark_ff::{BigInteger, PrimeField};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

use super::types::{abi_type_signature, AbiParameter};
use crate::hash::poseidon2_hash_bytes;
use crate::types::Fr;
use crate::Error;

fn strip_0x(s: &str) -> &str {
    s.strip_prefix("0x").unwrap_or(s)
}

fn decode_selector_hex(s: &str) -> Result<[u8; 4], Error> {
    let raw = strip_0x(s);
    if raw.len() > 8 {
        return Err(Error::InvalidData(
            "function selector must fit in 4 bytes".to_owned(),
        ));
    }
    let padded = format!("{raw:0>8}");
    let bytes = hex::decode(padded).map_err(|e| Error::InvalidData(e.to_string()))?;
    let mut out = [0u8; 4];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn field_to_selector_bytes(field: Fr) -> [u8; 4] {
    let raw = field.0.into_bigint().to_bytes_be();
    let mut padded = [0u8; 32];
    padded[32 - raw.len()..].copy_from_slice(&raw);
    let mut out = [0u8; 4];
    out.copy_from_slice(&padded[28..]);
    out
}

fn selector_bytes_to_field(bytes: [u8; 4]) -> Fr {
    Fr::from(u64::from(u32::from_be_bytes(bytes)))
}

fn selector_from_signature(signature: &str) -> [u8; 4] {
    let hash = poseidon2_hash_bytes(signature.as_bytes());
    field_to_selector_bytes(hash)
}

/// A 4-byte function selector used to identify contract functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FunctionSelector(pub [u8; 4]);

impl FunctionSelector {
    /// Parse a function selector from a hex string (e.g. `"0xaabbccdd"`).
    pub fn from_hex(value: &str) -> Result<Self, Error> {
        Ok(Self(decode_selector_hex(value)?))
    }

    /// Convert a field element to a function selector using its low 32 bits.
    pub fn from_field(field: Fr) -> Self {
        Self(field_to_selector_bytes(field))
    }

    /// Compute a function selector from a Noir function signature string.
    ///
    /// Aztec computes selectors by Poseidon2-hashing the raw signature bytes and
    /// taking the low 32 bits of the resulting field element.
    ///
    /// # Example
    /// ```
    /// # use aztec_core::abi::FunctionSelector;
    /// let selector = FunctionSelector::from_signature("sponsor_unconditionally()");
    /// ```
    pub fn from_signature(signature: &str) -> Self {
        Self(selector_from_signature(signature))
    }

    /// Derive a function selector from a function name and its ABI parameters.
    ///
    /// Constructs the canonical Noir signature (e.g., `transfer(Field,Field,u64)`)
    /// and computes the Poseidon2-based selector from it.
    pub fn from_name_and_parameters(name: &str, params: &[AbiParameter]) -> Self {
        let param_sigs: Vec<String> = params.iter().map(|p| abi_type_signature(&p.typ)).collect();
        let sig = format!("{}({})", name, param_sigs.join(","));
        Self::from_signature(&sig)
    }

    /// Convert this selector to its field representation.
    pub fn to_field(self) -> Fr {
        selector_bytes_to_field(self.0)
    }
}

impl fmt::Display for FunctionSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

impl Serialize for FunctionSelector {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for FunctionSelector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

/// A 4-byte authorization selector used to identify authwit request types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AuthorizationSelector(pub [u8; 4]);

impl AuthorizationSelector {
    /// Parse an authorization selector from a hex string.
    pub fn from_hex(value: &str) -> Result<Self, Error> {
        Ok(Self(decode_selector_hex(value)?))
    }

    /// Convert a field element to an authorization selector using its low 32 bits.
    pub fn from_field(field: Fr) -> Self {
        Self(field_to_selector_bytes(field))
    }

    /// Compute an authorization selector from an authorization signature.
    pub fn from_signature(signature: &str) -> Self {
        Self(selector_from_signature(signature))
    }

    /// Convert this selector to its field representation.
    pub fn to_field(self) -> Fr {
        selector_bytes_to_field(self.0)
    }
}

impl fmt::Display for AuthorizationSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
    }
}

impl Serialize for AuthorizationSelector {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for AuthorizationSelector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

/// A field-element event selector used to identify contract events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSelector(pub Fr);

/// A 7-bit note selector identifying a note type within a contract.
///
/// Valid values are 0..127 (fits in 7 bits). Assigned at compile time,
/// not derived from a hash like `FunctionSelector`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NoteSelector(pub u8);

impl NoteSelector {
    /// Maximum valid note selector value (2^7 - 1 = 127).
    pub const MAX_VALUE: u8 = 127;

    /// Create a new NoteSelector, returning an error if value >= 128.
    pub fn new(value: u8) -> Result<Self, Error> {
        if value > Self::MAX_VALUE {
            return Err(Error::InvalidData(format!(
                "note selector must fit in 7 bits (got {})",
                value
            )));
        }
        Ok(Self(value))
    }

    /// The empty/zero note selector.
    pub fn empty() -> Self {
        Self(0)
    }

    /// Convert from a field element.
    pub fn from_field(field: Fr) -> Result<Self, Error> {
        let val = field.to_usize();
        if val > Self::MAX_VALUE as usize {
            return Err(Error::InvalidData(format!(
                "note selector must fit in 7 bits (got {})",
                val
            )));
        }
        Ok(Self(val as u8))
    }

    /// Convert to a field element.
    pub fn to_field(self) -> Fr {
        Fr::from(self.0 as u64)
    }

    /// Parse from a hex string (e.g. `"0x1a"` or `"1a"`).
    pub fn from_hex(value: &str) -> Result<Self, Error> {
        let raw = value.strip_prefix("0x").unwrap_or(value);
        let val = u8::from_str_radix(raw, 16).map_err(|e| Error::InvalidData(e.to_string()))?;
        Self::new(val)
    }
}

impl fmt::Display for NoteSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:02x}", self.0)
    }
}

impl Serialize for NoteSelector {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.0)
    }
}

impl<'de> Deserialize<'de> for NoteSelector {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let val = u8::deserialize(deserializer)?;
        Self::new(val).map_err(serde::de::Error::custom)
    }
}
