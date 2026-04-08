use super::types::{AbiType, ContractArtifact, FunctionArtifact};

/// Returns true if the ABI type represents an AztecAddress or EthAddress struct.
pub fn is_address_struct(typ: &AbiType) -> bool {
    is_aztec_address_struct(typ) || is_eth_address_struct(typ)
}

/// Returns true if the ABI type is an `AztecAddress` struct.
pub fn is_aztec_address_struct(typ: &AbiType) -> bool {
    matches!(typ, AbiType::Struct { name, .. } if name.ends_with("address::AztecAddress"))
}

/// Returns true if the ABI type is an `EthAddress` struct.
pub fn is_eth_address_struct(typ: &AbiType) -> bool {
    matches!(typ, AbiType::Struct { name, .. } if name.ends_with("address::EthAddress"))
}

/// Returns true if the ABI type is a `FunctionSelector` struct.
pub fn is_function_selector_struct(typ: &AbiType) -> bool {
    matches!(typ, AbiType::Struct { name, .. }
        if name.ends_with("function_selector::FunctionSelector"))
}

/// Returns true if the ABI type is a struct wrapping a single `inner: Field`.
pub fn is_wrapped_field_struct(typ: &AbiType) -> bool {
    matches!(typ, AbiType::Struct { fields, .. }
        if fields.len() == 1
           && fields[0].name == "inner"
           && fields[0].typ == AbiType::Field)
}

/// Returns true if the ABI type is a `PublicKeys` struct (Noir ABI representation).
pub fn is_public_keys_struct(typ: &AbiType) -> bool {
    matches!(typ, AbiType::Struct { name, fields, .. }
        if name.ends_with("public_keys::PublicKeys")
           && fields.len() == 4
           && fields[0].name == "npk_m"
           && fields[1].name == "ivpk_m"
           && fields[2].name == "ovpk_m"
           && fields[3].name == "tpk_m")
}

/// Returns true if the ABI type is a `BoundedVec` struct.
pub fn is_bounded_vec_struct(typ: &AbiType) -> bool {
    matches!(typ, AbiType::Struct { name, fields, .. }
        if name.ends_with("bounded_vec::BoundedVec")
           && fields.len() == 2
           && fields[0].name == "storage"
           && fields[1].name == "len")
}

/// Returns true if the ABI type is an `Option` struct.
pub fn is_option_struct(typ: &AbiType) -> bool {
    matches!(typ, AbiType::Struct { name, fields, .. }
        if name.ends_with("option::Option")
           && fields.len() == 2
           && fields[0].name == "_is_some"
           && fields[1].name == "_value")
}

/// Compute the number of field elements an ABI type occupies when flattened.
pub fn abi_type_size(typ: &AbiType) -> usize {
    match typ {
        AbiType::Field | AbiType::Boolean | AbiType::Integer { .. } => 1,
        AbiType::String { length } => *length,
        AbiType::Array { element, length } => length * abi_type_size(element),
        AbiType::Struct { fields, .. } => fields.iter().map(|f| abi_type_size(&f.typ)).sum(),
        AbiType::Tuple { elements } => elements.iter().map(abi_type_size).sum(),
    }
}

/// Compute the total flattened field-element count for a function's parameters.
pub fn count_arguments_size(function: &FunctionArtifact) -> usize {
    function
        .parameters
        .iter()
        .map(|p| abi_type_size(&p.typ))
        .sum()
}

/// Validate a single ABI type recursively.
///
/// Returns a list of validation errors found at or below `path`.
fn validate_abi_type(typ: &AbiType, path: &str) -> Vec<String> {
    match typ {
        AbiType::Field | AbiType::Boolean => vec![],
        AbiType::Integer { width, .. } => {
            if *width == 0 {
                vec![format!("{path}: integer width must be > 0")]
            } else {
                vec![]
            }
        }
        AbiType::String { length } => {
            if *length == 0 {
                vec![format!("{path}: string length must be > 0")]
            } else {
                vec![]
            }
        }
        AbiType::Array { element, length } => {
            let mut errors = Vec::new();
            if *length == 0 {
                errors.push(format!("{path}: array length must be > 0"));
            }
            errors.extend(validate_abi_type(element, &format!("{path}[]")));
            errors
        }
        AbiType::Struct { name, fields } => {
            let mut errors = Vec::new();
            if name.is_empty() {
                errors.push(format!("{path}: struct name must not be empty"));
            }
            if fields.is_empty() {
                errors.push(format!("{path}: struct '{name}' must have at least one field"));
            }
            for field in fields {
                errors.extend(validate_abi_type(
                    &field.typ,
                    &format!("{path}.{}", field.name),
                ));
            }
            errors
        }
        AbiType::Tuple { elements } => {
            let mut errors = Vec::new();
            if elements.is_empty() {
                errors.push(format!("{path}: tuple must have at least one element"));
            }
            for (i, elem) in elements.iter().enumerate() {
                errors.extend(validate_abi_type(elem, &format!("{path}.{i}")));
            }
            errors
        }
    }
}

/// Validate a contract artifact's ABI for correctness.
///
/// Checks:
/// - At least one function exists
/// - All functions have names and valid selectors
/// - All parameter types are well-formed
/// - A constructor (initializer) exists
///
/// Returns a list of validation errors (empty = valid).
pub fn abi_checker(artifact: &ContractArtifact) -> Vec<String> {
    let mut errors = Vec::new();

    if artifact.functions.is_empty() {
        errors.push("artifact has no functions".to_owned());
        return errors;
    }

    let has_constructor = artifact.functions.iter().any(|f| f.is_initializer);
    if !has_constructor {
        errors.push("artifact has no constructor (initializer)".to_owned());
    }

    for func in &artifact.functions {
        let fn_path = format!("function '{}'", func.name);

        if func.name.is_empty() {
            errors.push(format!("{fn_path}: function name must not be empty"));
        }

        if func.selector.is_none() {
            errors.push(format!("{fn_path}: missing selector"));
        }

        for param in &func.parameters {
            errors.extend(validate_abi_type(
                &param.typ,
                &format!("{fn_path}.{}", param.name),
            ));
        }

        for (i, ret) in func.return_types.iter().enumerate() {
            errors.extend(validate_abi_type(ret, &format!("{fn_path}->return[{i}]")));
        }
    }

    errors
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::abi::types::AbiParameter;

    fn valid_artifact() -> ContractArtifact {
        ContractArtifact::from_json(
            r#"{
                "name": "TestContract",
                "functions": [
                    {
                        "name": "constructor",
                        "function_type": "private",
                        "is_initializer": true,
                        "is_static": false,
                        "parameters": [
                            { "name": "admin", "type": { "kind": "field" } }
                        ],
                        "return_types": [],
                        "selector": "0xe5fb6c81"
                    },
                    {
                        "name": "transfer",
                        "function_type": "private",
                        "is_initializer": false,
                        "is_static": false,
                        "parameters": [
                            { "name": "from", "type": { "kind": "field" } },
                            { "name": "to", "type": { "kind": "field" } }
                        ],
                        "return_types": [],
                        "selector": "0xd6f42325"
                    }
                ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn abi_checker_valid_artifact() {
        let errors = abi_checker(&valid_artifact());
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    #[test]
    fn abi_checker_empty_functions() {
        let artifact = ContractArtifact {
            name: "Empty".to_owned(),
            functions: vec![],
            outputs: None,
            file_map: None,
        };
        let errors = abi_checker(&artifact);
        assert!(!errors.is_empty());
        assert!(errors[0].contains("no functions"));
    }

    #[test]
    fn abi_checker_invalid_parameter_type() {
        let mut artifact = valid_artifact();
        artifact.functions[1].parameters.push(AbiParameter {
            name: "bad".to_owned(),
            typ: AbiType::Integer {
                sign: "unsigned".to_owned(),
                width: 0,
            },
            visibility: None,
        });
        let errors = abi_checker(&artifact);
        assert!(!errors.is_empty());
        assert!(
            errors.iter().any(|e| e.contains("width must be > 0")),
            "errors: {errors:?}"
        );
    }

    #[test]
    fn abi_checker_missing_constructor() {
        let mut artifact = valid_artifact();
        for func in &mut artifact.functions {
            func.is_initializer = false;
        }
        let errors = abi_checker(&artifact);
        assert!(
            errors.iter().any(|e| e.contains("no constructor")),
            "errors: {errors:?}"
        );
    }
}
