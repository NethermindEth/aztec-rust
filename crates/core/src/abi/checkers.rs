use super::types::{AbiType, FunctionArtifact};

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
