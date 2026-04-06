use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FunctionSelector(pub [u8; 4]);

impl FunctionSelector {
    pub fn from_hex(value: &str) -> Result<Self, Error> {
        Ok(Self(decode_selector_hex(value)?))
    }

    pub fn from_name(_name: &str) -> Result<Self, Error> {
        Err(Error::Abi(
            "function selector derivation is not implemented yet".to_owned(),
        ))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSelector(pub Fr);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FunctionType {
    Private,
    Public,
    Utility,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AbiType {
    Field,
    Boolean,
    Integer { sign: String, width: u16 },
    Array { element: Box<Self>, length: usize },
    String { length: usize },
    Struct {
        name: String,
        fields: Vec<AbiParameter>,
    },
    Tuple { elements: Vec<Self> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AbiValue {
    Field(Fr),
    Boolean(bool),
    Integer(i128),
    Array(Vec<Self>),
    String(String),
    Struct(BTreeMap<String, Self>),
    Tuple(Vec<Self>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbiParameter {
    pub name: String,
    #[serde(rename = "type")]
    pub typ: AbiType,
    #[serde(default)]
    pub visibility: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionArtifact {
    pub name: String,
    pub function_type: FunctionType,
    #[serde(default)]
    pub is_initializer: bool,
    #[serde(default)]
    pub is_static: bool,
    #[serde(default)]
    pub parameters: Vec<AbiParameter>,
    #[serde(default)]
    pub return_types: Vec<AbiType>,
    #[serde(default)]
    pub selector: Option<FunctionSelector>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractArtifact {
    pub name: String,
    #[serde(default)]
    pub functions: Vec<FunctionArtifact>,
}

impl ContractArtifact {
    pub fn from_json(json: &str) -> Result<Self, Error> {
        serde_json::from_str(json).map_err(Error::from)
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn function_type_roundtrip() {
        let json = match serde_json::to_string(&FunctionType::Private) {
            Ok(json) => json,
            Err(err) => panic!("serializing FunctionType::Private should succeed: {err}"),
        };
        assert_eq!(json, "\"private\"");
        let decoded: FunctionType = match serde_json::from_str(&json) {
            Ok(decoded) => decoded,
            Err(err) => panic!("deserializing FunctionType should succeed: {err}"),
        };
        assert_eq!(decoded, FunctionType::Private);
    }

    #[test]
    fn load_minimal_artifact() {
        let json = r#"
        {
          "name": "TestContract",
          "functions": [
            {
              "name": "increment",
              "function_type": "public",
              "is_initializer": false,
              "is_static": false,
              "parameters": [
                { "name": "value", "type": { "kind": "field" } }
              ],
              "return_types": []
            }
          ]
        }
        "#;

        let artifact = match ContractArtifact::from_json(json) {
            Ok(artifact) => artifact,
            Err(err) => panic!("loading minimal contract artifact should succeed: {err}"),
        };
        assert_eq!(artifact.name, "TestContract");
        assert_eq!(artifact.functions.len(), 1);
        assert_eq!(artifact.functions[0].name, "increment");
    }

    #[test]
    fn abi_value_field_serializes() {
        let value = AbiValue::Field(Fr::from(1u64));
        let json = match serde_json::to_string(&value) {
            Ok(json) => json,
            Err(err) => panic!("serializing AbiValue::Field should succeed: {err}"),
        };
        assert!(json.contains("field"));
        assert!(json.contains("0000000000000000000000000000000000000000000000000000000000000001"));
    }
}
