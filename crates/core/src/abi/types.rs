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
    /// Size of the PrivateContextInputs parameter for each private function.
    /// Computed during nargo parsing and persisted with the artifact so PXE can
    /// reconstruct private-function witnesses after store roundtrips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_inputs_sizes: Option<std::collections::HashMap<String, usize>>,
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

    /// Parse a contract artifact from raw nargo compiler output.
    ///
    /// The nargo compiler output has a different structure than the Aztec-processed
    /// format. This method handles the conversion:
    /// - Strips `__aztec_nr_internals__` prefix from function names
    /// - Maps `custom_attributes` to `function_type` (Private/Public/Utility)
    /// - Extracts parameters from `abi.parameters`
    /// - Computes function selectors from name + parameters
    /// - Filters out the `inputs` parameter (PrivateContextInputs) from private functions
    pub fn from_nargo_json(json: &str) -> Result<Self, Error> {
        let raw: serde_json::Value = serde_json::from_str(json).map_err(Error::from)?;

        let name = raw
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_owned();

        let outputs = raw.get("outputs").cloned();
        let file_map = raw.get("file_map").cloned();

        let raw_functions = raw
            .get("functions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut functions = Vec::new();
        let mut context_inputs_sizes = std::collections::HashMap::new();

        for raw_fn in &raw_functions {
            if let Some(func) = Self::parse_nargo_function(raw_fn) {
                // Compute PrivateContextInputs size for private functions
                if func.function_type == FunctionType::Private {
                    let abi = raw_fn.get("abi");
                    let raw_params = abi
                        .and_then(|a| a.get("parameters"))
                        .and_then(|p| p.as_array());
                    if let Some(params) = raw_params {
                        for p in params {
                            let param_name = p.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            if param_name == "inputs" {
                                let path = p
                                    .pointer("/type/path")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                if path.contains("PrivateContextInputs") {
                                    if let Some(typ) =
                                        p.get("type").and_then(Self::parse_nargo_type)
                                    {
                                        let size = Self::count_abi_type_fields(&typ);
                                        context_inputs_sizes.insert(func.name.clone(), size);
                                    }
                                }
                            }
                        }
                    }
                }
                functions.push(func);
            }
        }

        Ok(Self {
            name,
            functions,
            outputs,
            file_map,
            context_inputs_sizes: Some(context_inputs_sizes),
        })
    }

    /// Parse a single function from raw nargo output.
    fn parse_nargo_function(raw: &serde_json::Value) -> Option<FunctionArtifact> {
        let raw_name = raw.get("name")?.as_str()?;
        let attrs: Vec<String> = raw
            .get("custom_attributes")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let is_unconstrained = raw
            .get("is_unconstrained")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Determine function type from custom_attributes
        let function_type = if attrs.iter().any(|a| a == "abi_private") {
            FunctionType::Private
        } else if attrs.iter().any(|a| a == "abi_utility") {
            FunctionType::Utility
        } else if attrs.iter().any(|a| a == "abi_public") {
            FunctionType::Public
        } else if is_unconstrained {
            FunctionType::Utility
        } else {
            FunctionType::Private
        };

        let is_initializer = attrs.iter().any(|a| a == "abi_initializer");
        let is_static = attrs.iter().any(|a| a == "abi_view");
        let is_only_self = if attrs.iter().any(|a| a == "abi_only_self") {
            Some(true)
        } else {
            None
        };

        // Strip __aztec_nr_internals__ prefix
        let name = raw_name
            .strip_prefix("__aztec_nr_internals__")
            .or_else(|| raw_name.strip_prefix("__aztec_nr_internals___"))
            .unwrap_or(raw_name)
            .to_owned();

        // Extract parameters from abi.parameters, filtering out PrivateContextInputs
        let abi = raw.get("abi");
        let raw_params = abi
            .and_then(|a| a.get("parameters"))
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();

        let parameters: Vec<AbiParameter> = raw_params
            .iter()
            .filter(|p| {
                // Filter out the PrivateContextInputs parameter for private functions
                let param_name = p.get("name").and_then(|n| n.as_str()).unwrap_or("");
                if param_name == "inputs" && function_type == FunctionType::Private {
                    // Check if this is the PrivateContextInputs struct
                    let path = p
                        .pointer("/type/path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    return !path.contains("PrivateContextInputs");
                }
                true
            })
            .filter_map(|p| Self::parse_nargo_parameter(p))
            .collect();

        // Extract return types
        let return_types = abi
            .and_then(|a| a.get("return_type"))
            .and_then(|rt| rt.get("abi_type"))
            .and_then(|t| Self::parse_nargo_type(t))
            .map(|t| vec![t])
            .unwrap_or_default();

        // Get bytecode
        let bytecode = raw
            .get("bytecode")
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned());

        let verification_key = raw
            .get("verification_key")
            .or_else(|| raw.get("verificationKey"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned());
        let verification_key_hash = raw
            .get("verification_key_hash")
            .or_else(|| raw.get("verificationKeyHash"))
            .and_then(|value| serde_json::from_value(value.clone()).ok());

        // Get debug symbols
        let debug_symbols = raw.get("debug_symbols").cloned();

        // Compute selector from name + parameters
        let selector = Some(super::FunctionSelector::from_name_and_parameters(
            &name,
            &parameters,
        ));

        Some(FunctionArtifact {
            name,
            function_type,
            is_initializer,
            is_static,
            is_only_self,
            parameters,
            return_types,
            error_types: abi.and_then(|a| a.get("error_types")).cloned(),
            selector,
            bytecode,
            verification_key_hash,
            verification_key,
            custom_attributes: Some(attrs),
            is_unconstrained: Some(is_unconstrained),
            debug_symbols,
        })
    }

    /// Parse a parameter from nargo ABI format.
    fn parse_nargo_parameter(raw: &serde_json::Value) -> Option<AbiParameter> {
        let name = raw.get("name")?.as_str()?.to_owned();
        let typ = raw.get("type").and_then(Self::parse_nargo_type)?;
        let visibility = raw
            .get("visibility")
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned());
        Some(AbiParameter {
            name,
            typ,
            visibility,
        })
    }

    /// Parse an ABI type from nargo format.
    fn parse_nargo_type(raw: &serde_json::Value) -> Option<AbiType> {
        let kind = raw.get("kind")?.as_str()?;
        match kind {
            "field" => Some(AbiType::Field),
            "boolean" => Some(AbiType::Boolean),
            "integer" => {
                let sign = raw
                    .get("sign")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unsigned")
                    .to_owned();
                let width = raw.get("width").and_then(|v| v.as_u64()).unwrap_or(32) as u16;
                Some(AbiType::Integer { sign, width })
            }
            "array" => {
                let element = raw
                    .get("type")
                    .or_else(|| raw.get("element"))
                    .and_then(Self::parse_nargo_type)?;
                let length = raw.get("length").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                Some(AbiType::Array {
                    element: Box::new(element),
                    length,
                })
            }
            "string" => {
                let length = raw.get("length").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                Some(AbiType::String { length })
            }
            "struct" => {
                let name = raw
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_owned();
                let raw_fields = raw.get("fields").and_then(|v| v.as_array())?;
                let fields: Vec<AbiParameter> = raw_fields
                    .iter()
                    .filter_map(|f| Self::parse_nargo_parameter(f))
                    .collect();
                Some(AbiType::Struct { name, fields })
            }
            "tuple" => {
                let raw_elements = raw
                    .get("fields")
                    .or_else(|| raw.get("elements"))
                    .and_then(|v| v.as_array())?;
                let elements: Vec<AbiType> = raw_elements
                    .iter()
                    .filter_map(Self::parse_nargo_type)
                    .collect();
                Some(AbiType::Tuple { elements })
            }
            _ => {
                // Fallback: treat as field
                Some(AbiType::Field)
            }
        }
    }

    /// Find a function by selector.
    pub fn find_function_by_selector(
        &self,
        selector: &super::FunctionSelector,
    ) -> Option<&FunctionArtifact> {
        self.functions
            .iter()
            .find(|f| f.selector.as_ref() == Some(selector))
    }

    /// Count the number of field elements the PrivateContextInputs parameter
    /// occupies for a given function. Returns 0 if the function has no such
    /// parameter (public/utility functions).
    ///
    /// This is needed because compiled Noir bytecode expects the full
    /// PrivateContextInputs flattened into the initial witness before user args.
    pub fn private_context_inputs_size(&self, function_name: &str) -> usize {
        self.context_inputs_sizes
            .as_ref()
            .and_then(|m| m.get(function_name).copied())
            .unwrap_or(0)
    }

    /// Count the number of scalar fields an ABI type flattens to.
    pub fn count_abi_type_fields(typ: &AbiType) -> usize {
        match typ {
            AbiType::Field | AbiType::Boolean => 1,
            AbiType::Integer { .. } => 1,
            AbiType::Array { element, length } => Self::count_abi_type_fields(element) * length,
            AbiType::String { length } => *length,
            AbiType::Struct { fields, .. } => fields
                .iter()
                .map(|f| Self::count_abi_type_fields(&f.typ))
                .sum(),
            AbiType::Tuple { elements } => elements.iter().map(Self::count_abi_type_fields).sum(),
        }
    }
}
