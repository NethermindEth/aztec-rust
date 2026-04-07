use std::collections::BTreeMap;

use super::checkers::{
    abi_type_size, is_aztec_address_struct, is_bounded_vec_struct, is_eth_address_struct,
    is_option_struct,
};
use super::types::AbiType;
use crate::types::Fr;
use crate::Error;

/// A decoded ABI value, produced by `decode_from_abi`.
#[derive(Clone, Debug, PartialEq)]
pub enum AbiDecoded {
    /// A field element.
    Field(Fr),
    /// A boolean value.
    Boolean(bool),
    /// An integer (signed or unsigned, stored as i128).
    Integer(i128),
    /// A fixed-length array of decoded values.
    Array(Vec<AbiDecoded>),
    /// A decoded string.
    String(String),
    /// A struct with named fields.
    Struct(BTreeMap<String, AbiDecoded>),
    /// A tuple of decoded values.
    Tuple(Vec<AbiDecoded>),
    /// An AztecAddress (special-cased from struct decoding).
    Address(crate::types::AztecAddress),
    /// None / absent (decoded from Option with `_is_some == false`).
    None,
}

/// Decode field elements back into typed ABI values.
///
/// `types` is the list of return types from a function's ABI.
/// `fields` is the flat array of field elements returned by the function.
pub fn decode_from_abi(types: &[AbiType], fields: &[Fr]) -> Result<AbiDecoded, Error> {
    let mut cursor = 0;
    if types.len() == 1 {
        return decode_next(&types[0], fields, &mut cursor);
    }
    let results: Vec<AbiDecoded> = types
        .iter()
        .map(|t| decode_next(t, fields, &mut cursor))
        .collect::<Result<_, _>>()?;
    Ok(AbiDecoded::Tuple(results))
}

fn take(fields: &[Fr], cursor: &mut usize) -> Result<Fr, Error> {
    if *cursor >= fields.len() {
        return Err(Error::Abi(format!(
            "insufficient fields for decoding: cursor {} >= len {}",
            cursor,
            fields.len()
        )));
    }
    let fr = fields[*cursor];
    *cursor += 1;
    Ok(fr)
}

fn decode_next(typ: &AbiType, fields: &[Fr], cursor: &mut usize) -> Result<AbiDecoded, Error> {
    match typ {
        AbiType::Field => {
            let fr = take(fields, cursor)?;
            Ok(AbiDecoded::Field(fr))
        }
        AbiType::Boolean => {
            let fr = take(fields, cursor)?;
            Ok(AbiDecoded::Boolean(!fr.is_zero()))
        }
        AbiType::Integer { sign, width } => {
            let fr = take(fields, cursor)?;
            let bytes = fr.to_be_bytes();
            let raw = u128::from_be_bytes(bytes[16..].try_into().unwrap());
            if sign == "signed" {
                let value = if *width >= 128 {
                    // For 128-bit signed, u128-to-i128 reinterpretation handles
                    // two's complement naturally (values >= 2^127 become negative).
                    raw as i128
                } else if raw >= (1u128 << (*width - 1)) {
                    raw as i128 - (1i128 << *width)
                } else {
                    raw as i128
                };
                Ok(AbiDecoded::Integer(value))
            } else {
                Ok(AbiDecoded::Integer(raw as i128))
            }
        }
        AbiType::Array { element, length } => {
            let mut items = Vec::with_capacity(*length);
            for _ in 0..*length {
                items.push(decode_next(element, fields, cursor)?);
            }
            Ok(AbiDecoded::Array(items))
        }
        AbiType::String { length } => {
            let mut chars = Vec::with_capacity(*length);
            for _ in 0..*length {
                let fr = take(fields, cursor)?;
                let byte = fr.to_usize() as u8;
                if byte == 0 {
                    // Skip null terminators but still consume remaining fields
                    for _ in (chars.len() + 1)..*length {
                        take(fields, cursor)?;
                    }
                    break;
                }
                chars.push(char::from(byte));
            }
            Ok(AbiDecoded::String(chars.into_iter().collect()))
        }
        AbiType::Struct {
            fields: struct_fields,
            ..
        } => {
            if is_aztec_address_struct(typ) {
                let fr = take(fields, cursor)?;
                Ok(AbiDecoded::Address(crate::types::AztecAddress(fr)))
            } else if is_eth_address_struct(typ) {
                let fr = take(fields, cursor)?;
                Ok(AbiDecoded::Field(fr))
            } else if is_option_struct(typ) {
                let is_some_fr = take(fields, cursor)?;
                if is_some_fr.is_zero() {
                    // Skip _value fields
                    let value_size = abi_type_size(&struct_fields[1].typ);
                    for _ in 0..value_size {
                        take(fields, cursor)?;
                    }
                    Ok(AbiDecoded::None)
                } else {
                    let value = decode_next(&struct_fields[1].typ, fields, cursor)?;
                    Ok(value)
                }
            } else if is_bounded_vec_struct(typ) {
                let (elem_type, max_len) = match &struct_fields[0].typ {
                    AbiType::Array { element, length } => (element.as_ref(), *length),
                    _ => return Err(Error::Abi("BoundedVec storage must be an Array".into())),
                };
                let mut items = Vec::with_capacity(max_len);
                for _ in 0..max_len {
                    items.push(decode_next(elem_type, fields, cursor)?);
                }
                let len_fr = take(fields, cursor)?;
                let actual_len = len_fr.to_usize();
                items.truncate(actual_len);
                Ok(AbiDecoded::Array(items))
            } else {
                // Generic struct decoding
                let mut map = BTreeMap::new();
                for field in struct_fields {
                    let value = decode_next(&field.typ, fields, cursor)?;
                    map.insert(field.name.clone(), value);
                }
                Ok(AbiDecoded::Struct(map))
            }
        }
        AbiType::Tuple { elements } => {
            let mut items = Vec::with_capacity(elements.len());
            for elem in elements {
                items.push(decode_next(elem, fields, cursor)?);
            }
            Ok(AbiDecoded::Tuple(items))
        }
    }
}
