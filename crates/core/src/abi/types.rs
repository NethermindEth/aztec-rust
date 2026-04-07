use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::types::Fr;
use crate::Error;

/// Convert an ABI type to its canonical Noir signature representation.
pub fn abi_type_signature(typ: &AbiType) -> String {
    match typ {
        AbiType::Field => "Field".to_owned(),
        AbiType::Boolean => "bool".to_owned(),
        AbiType::Integer { sign, width } => {
            let prefix = if sign == "signed" { "i" } else { "u" };
            format!("{prefix}{width}")
        }
        AbiType::Array { element, length } => {
            format!("[{};{length}]", abi_type_signature(element))
        }
        AbiType::String { length } => format!("str<{length}>"),
        AbiType::Struct { fields, .. } => {
            let inner: Vec<String> = fields.iter().map(|f| abi_type_signature(&f.typ)).collect();
            format!("({})", inner.join(","))
        }
        AbiType::Tuple { elements } => {
            let inner: Vec<String> = elements.iter().map(abi_type_signature).collect();
            format!("({})", inner.join(","))
        }
    }
}

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
        #[serde(rename = "type", alias = "element")]
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
        /// Struct type name / path.
        #[serde(alias = "path")]
        name: String,
        /// Struct fields.
        fields: Vec<AbiParameter>,
    },
    /// An anonymous tuple of types.
    Tuple {
        /// Element types.
        #[serde(alias = "fields")]
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
    #[serde(alias = "functionType")]
    pub function_type: FunctionType,
    /// Whether this function is a contract initializer (constructor).
    #[serde(default, alias = "isInitializer")]
    pub is_initializer: bool,
    /// Whether this function is a static (read-only) call.
    #[serde(default, alias = "isStatic")]
    pub is_static: bool,
    /// Whether this function is only callable by the contract itself.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "isOnlySelf")]
    pub is_only_self: Option<bool>,
    /// Function parameters.
    #[serde(default)]
    pub parameters: Vec<AbiParameter>,
    /// Return types.
    #[serde(default, alias = "returnTypes")]
    pub return_types: Vec<AbiType>,
    /// Error types thrown by the function.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "errorTypes")]
    pub error_types: Option<serde_json::Value>,
    /// Pre-computed function selector.
    #[serde(default)]
    pub selector: Option<super::FunctionSelector>,
    /// Compiled bytecode (base64 or hex encoded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytecode: Option<String>,
    /// Hash of the verification key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_key_hash: Option<Fr>,
    /// Raw verification key (base64 or hex encoded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_key: Option<String>,
    /// Custom attributes / annotations from the Noir source.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "customAttributes"
    )]
    pub custom_attributes: Option<Vec<String>>,
    /// Whether this is an unconstrained function.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "isUnconstrained"
    )]
    pub is_unconstrained: Option<bool>,
    /// Debug symbols (opaque JSON).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "debugSymbols"
    )]
    pub debug_symbols: Option<serde_json::Value>,
}

/// A deserialized contract artifact containing function metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractArtifact {
    /// Contract name.
    pub name: String,
    /// Functions defined in the contract.
    #[serde(default)]
    pub functions: Vec<FunctionArtifact>,
    /// Compiler output metadata (opaque JSON).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outputs: Option<serde_json::Value>,
    /// Source file map (opaque JSON).
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "fileMap")]
    pub file_map: Option<serde_json::Value>,
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

    /// Serialize this artifact to a JSON byte buffer.
    pub fn to_buffer(&self) -> Result<Vec<u8>, Error> {
        serde_json::to_vec(self).map_err(Error::from)
    }

    /// Deserialize an artifact from a JSON byte buffer.
    pub fn from_buffer(buffer: &[u8]) -> Result<Self, Error> {
        serde_json::from_slice(buffer).map_err(Error::from)
    }

    /// Serialize to a pretty-printed JSON string.
    pub fn to_json(&self) -> Result<String, Error> {
        serde_json::to_string_pretty(self).map_err(Error::from)
    }
}
