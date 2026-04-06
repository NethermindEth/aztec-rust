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

/// A 4-byte function selector used to identify contract functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FunctionSelector(pub [u8; 4]);

impl FunctionSelector {
    /// Parse a function selector from a hex string (e.g. `"0xaabbccdd"`).
    pub fn from_hex(value: &str) -> Result<Self, Error> {
        Ok(Self(decode_selector_hex(value)?))
    }

    /// Derive a function selector from a function name.
    ///
    /// Not yet implemented — returns an error.
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

/// A field-element event selector used to identify contract events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSelector(pub Fr);

/// The type of a contract function.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FunctionType {
    /// A private function executed in the user's PXE.
    Private,
    /// A public function executed by the sequencer.
    Public,
    /// A utility (view/unconstrained) function for read-only queries.
    Utility,
}

/// ABI type representation for function parameters and return values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AbiType {
    /// A BN254 field element.
    Field,
    /// A boolean value.
    Boolean,
    /// A signed or unsigned integer with a specific bit width.
    Integer {
        /// `"signed"` or `"unsigned"`.
        sign: String,
        /// Bit width of the integer.
        width: u16,
    },
    /// A fixed-length array of elements.
    Array {
        /// Element type.
        element: Box<Self>,
        /// Fixed array length.
        length: usize,
    },
    /// A fixed-length string.
    String {
        /// Maximum string length.
        length: usize,
    },
    /// A named struct with typed fields.
    Struct {
        /// Struct type name.
        name: String,
        /// Struct fields.
        fields: Vec<AbiParameter>,
    },
    /// An anonymous tuple of types.
    Tuple {
        /// Element types.
        elements: Vec<Self>,
    },
}

/// A concrete ABI value used as a function argument or return value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum AbiValue {
    /// A BN254 field element value.
    Field(Fr),
    /// A boolean value.
    Boolean(bool),
    /// An integer value.
    Integer(i128),
    /// An array of values.
    Array(Vec<Self>),
    /// A string value.
    String(String),
    /// A struct value with named fields.
    Struct(BTreeMap<String, Self>),
    /// A tuple of values.
    Tuple(Vec<Self>),
}

/// A named, typed parameter in a function ABI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbiParameter {
    /// Parameter name.
    pub name: String,
    /// Parameter type.
    #[serde(rename = "type")]
    pub typ: AbiType,
    /// Visibility (e.g. `"private"`, `"public"`).
    #[serde(default)]
    pub visibility: Option<String>,
}

/// Metadata for a single function within a contract artifact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionArtifact {
    /// Function name.
    pub name: String,
    /// Whether this is a private, public, or utility function.
    pub function_type: FunctionType,
    /// Whether this function is a contract initializer (constructor).
    #[serde(default)]
    pub is_initializer: bool,
    /// Whether this function is a static (read-only) call.
    #[serde(default)]
    pub is_static: bool,
    /// Function parameters.
    #[serde(default)]
    pub parameters: Vec<AbiParameter>,
    /// Return types.
    #[serde(default)]
    pub return_types: Vec<AbiType>,
    /// Pre-computed function selector.
    #[serde(default)]
    pub selector: Option<FunctionSelector>,
}

/// A deserialized contract artifact containing function metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractArtifact {
    /// Contract name.
    pub name: String,
    /// Functions defined in the contract.
    #[serde(default)]
    pub functions: Vec<FunctionArtifact>,
}

impl ContractArtifact {
    /// Deserialize a contract artifact from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, Error> {
        serde_json::from_str(json).map_err(Error::from)
    }

    /// Find a function by name, returning an error if not found.
    pub fn find_function(&self, name: &str) -> Result<&FunctionArtifact, Error> {
        self.functions
            .iter()
            .find(|f| f.name == name)
            .ok_or_else(|| {
                Error::Abi(format!(
                    "function '{}' not found in artifact '{}'",
                    name, self.name
                ))
            })
    }

    /// Find a function by name and type, returning an error if not found.
    pub fn find_function_by_type(
        &self,
        name: &str,
        function_type: &FunctionType,
    ) -> Result<&FunctionArtifact, Error> {
        self.functions
            .iter()
            .find(|f| f.name == name && &f.function_type == function_type)
            .ok_or_else(|| {
                Error::Abi(format!(
                    "{:?} function '{}' not found in artifact '{}'",
                    function_type, name, self.name
                ))
            })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const MINIMAL_ARTIFACT: &str = r#"
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

    const MULTI_FUNCTION_ARTIFACT: &str = r#"
    {
      "name": "TokenContract",
      "functions": [
        {
          "name": "constructor",
          "function_type": "private",
          "is_initializer": true,
          "is_static": false,
          "parameters": [
            { "name": "admin", "type": { "kind": "field" } },
            { "name": "name", "type": { "kind": "string", "length": 31 } }
          ],
          "return_types": []
        },
        {
          "name": "transfer",
          "function_type": "private",
          "is_initializer": false,
          "is_static": false,
          "parameters": [
            { "name": "from", "type": { "kind": "field" } },
            { "name": "to", "type": { "kind": "field" } },
            { "name": "amount", "type": { "kind": "integer", "sign": "unsigned", "width": 64 } }
          ],
          "return_types": []
        },
        {
          "name": "balance_of",
          "function_type": "utility",
          "is_initializer": false,
          "is_static": true,
          "parameters": [
            { "name": "owner", "type": { "kind": "field" } }
          ],
          "return_types": [
            { "kind": "integer", "sign": "unsigned", "width": 64 }
          ]
        },
        {
          "name": "total_supply",
          "function_type": "public",
          "is_initializer": false,
          "is_static": true,
          "parameters": [],
          "return_types": [
            { "kind": "integer", "sign": "unsigned", "width": 64 }
          ]
        }
      ]
    }
    "#;

    #[test]
    fn function_type_roundtrip() {
        for (ft, expected) in [
            (FunctionType::Private, "\"private\""),
            (FunctionType::Public, "\"public\""),
            (FunctionType::Utility, "\"utility\""),
        ] {
            let json = serde_json::to_string(&ft).expect("serialize FunctionType");
            assert_eq!(json, expected);
            let decoded: FunctionType =
                serde_json::from_str(&json).expect("deserialize FunctionType");
            assert_eq!(decoded, ft);
        }
    }

    #[test]
    fn function_selector_hex_roundtrip() {
        let selector = FunctionSelector::from_hex("0xaabbccdd").expect("valid hex");
        assert_eq!(selector.0, [0xaa, 0xbb, 0xcc, 0xdd]);
        assert_eq!(selector.to_string(), "0xaabbccdd");

        let json = serde_json::to_string(&selector).expect("serialize selector");
        let decoded: FunctionSelector = serde_json::from_str(&json).expect("deserialize selector");
        assert_eq!(decoded, selector);
    }

    #[test]
    fn function_selector_rejects_too_long() {
        let result = FunctionSelector::from_hex("0xaabbccddee");
        assert!(result.is_err());
    }

    #[test]
    fn event_selector_roundtrip() {
        let selector = EventSelector(Fr::from(42u64));
        let json = serde_json::to_string(&selector).expect("serialize EventSelector");
        let decoded: EventSelector =
            serde_json::from_str(&json).expect("deserialize EventSelector");
        assert_eq!(decoded, selector);
    }

    #[test]
    fn load_minimal_artifact() {
        let artifact = ContractArtifact::from_json(MINIMAL_ARTIFACT).expect("parse artifact");
        assert_eq!(artifact.name, "TestContract");
        assert_eq!(artifact.functions.len(), 1);
        assert_eq!(artifact.functions[0].name, "increment");
        assert_eq!(artifact.functions[0].function_type, FunctionType::Public);
        assert!(!artifact.functions[0].is_initializer);
        assert_eq!(artifact.functions[0].parameters.len(), 1);
        assert_eq!(artifact.functions[0].parameters[0].name, "value");
    }

    #[test]
    fn load_multi_function_artifact() {
        let artifact =
            ContractArtifact::from_json(MULTI_FUNCTION_ARTIFACT).expect("parse artifact");
        assert_eq!(artifact.name, "TokenContract");
        assert_eq!(artifact.functions.len(), 4);

        let constructor = &artifact.functions[0];
        assert_eq!(constructor.name, "constructor");
        assert_eq!(constructor.function_type, FunctionType::Private);
        assert!(constructor.is_initializer);
        assert_eq!(constructor.parameters.len(), 2);

        let transfer = &artifact.functions[1];
        assert_eq!(transfer.name, "transfer");
        assert_eq!(transfer.function_type, FunctionType::Private);
        assert!(!transfer.is_static);

        let balance = &artifact.functions[2];
        assert_eq!(balance.name, "balance_of");
        assert_eq!(balance.function_type, FunctionType::Utility);
        assert!(balance.is_static);
        assert_eq!(balance.return_types.len(), 1);

        let supply = &artifact.functions[3];
        assert_eq!(supply.name, "total_supply");
        assert_eq!(supply.function_type, FunctionType::Public);
        assert!(supply.is_static);
    }

    #[test]
    fn find_function_by_name() {
        let artifact =
            ContractArtifact::from_json(MULTI_FUNCTION_ARTIFACT).expect("parse artifact");

        let transfer = artifact.find_function("transfer").expect("find transfer");
        assert_eq!(transfer.name, "transfer");
        assert_eq!(transfer.function_type, FunctionType::Private);
    }

    #[test]
    fn find_function_not_found() {
        let artifact =
            ContractArtifact::from_json(MULTI_FUNCTION_ARTIFACT).expect("parse artifact");

        let result = artifact.find_function("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn find_function_by_type() {
        let artifact =
            ContractArtifact::from_json(MULTI_FUNCTION_ARTIFACT).expect("parse artifact");

        let balance = artifact
            .find_function_by_type("balance_of", &FunctionType::Utility)
            .expect("find balance_of as utility");
        assert_eq!(balance.name, "balance_of");

        let wrong_type = artifact.find_function_by_type("balance_of", &FunctionType::Public);
        assert!(wrong_type.is_err());
    }

    #[test]
    fn abi_value_field_roundtrip() {
        let value = AbiValue::Field(Fr::from(1u64));
        let json = serde_json::to_string(&value).expect("serialize AbiValue::Field");
        assert!(json.contains("field"));
        let decoded: AbiValue = serde_json::from_str(&json).expect("deserialize AbiValue");
        assert_eq!(decoded, value);
    }

    #[test]
    fn abi_value_boolean_roundtrip() {
        let value = AbiValue::Boolean(true);
        let json = serde_json::to_string(&value).expect("serialize");
        let decoded: AbiValue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, value);
    }

    #[test]
    fn abi_value_integer_roundtrip() {
        let value = AbiValue::Integer(42);
        let json = serde_json::to_string(&value).expect("serialize");
        let decoded: AbiValue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, value);
    }

    #[test]
    fn abi_value_array_roundtrip() {
        let value = AbiValue::Array(vec![
            AbiValue::Field(Fr::from(1u64)),
            AbiValue::Field(Fr::from(2u64)),
        ]);
        let json = serde_json::to_string(&value).expect("serialize");
        let decoded: AbiValue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, value);
    }

    #[test]
    fn abi_value_struct_roundtrip() {
        let mut fields = BTreeMap::new();
        fields.insert("x".to_owned(), AbiValue::Field(Fr::from(1u64)));
        fields.insert("y".to_owned(), AbiValue::Integer(2));
        let value = AbiValue::Struct(fields);
        let json = serde_json::to_string(&value).expect("serialize");
        let decoded: AbiValue = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, value);
    }

    #[test]
    fn abi_type_struct_roundtrip() {
        let typ = AbiType::Struct {
            name: "Point".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "x".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                },
                AbiParameter {
                    name: "y".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                },
            ],
        };
        let json = serde_json::to_string(&typ).expect("serialize AbiType::Struct");
        let decoded: AbiType = serde_json::from_str(&json).expect("deserialize AbiType::Struct");
        assert_eq!(decoded, typ);
    }

    #[test]
    fn abi_type_array_roundtrip() {
        let typ = AbiType::Array {
            element: Box::new(AbiType::Field),
            length: 10,
        };
        let json = serde_json::to_string(&typ).expect("serialize");
        let decoded: AbiType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, typ);
    }

    #[test]
    fn artifact_from_invalid_json_fails() {
        let result = ContractArtifact::from_json("not json");
        assert!(result.is_err());
    }
}
