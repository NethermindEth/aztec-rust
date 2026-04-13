use super::checkers::{
    abi_type_size, is_address_struct, is_bounded_vec_struct, is_function_selector_struct,
    is_option_struct, is_wrapped_field_struct,
};
use super::types::{AbiType, AbiValue, FunctionArtifact};
use crate::types::Fr;
use crate::Error;

/// ABI-encode a function's arguments into field elements.
pub fn encode_arguments(function: &FunctionArtifact, args: &[AbiValue]) -> Result<Vec<Fr>, Error> {
    if function.parameters.len() != args.len() {
        return Err(Error::Abi(format!(
            "function '{}' expects {} argument(s), got {}",
            function.name,
            function.parameters.len(),
            args.len()
        )));
    }

    let mut out = Vec::new();
    for (param, value) in function.parameters.iter().zip(args) {
        encode_value(&param.typ, value, &mut out)?;
    }
    Ok(out)
}

pub fn encode_value(typ: &AbiType, value: &AbiValue, out: &mut Vec<Fr>) -> Result<(), Error> {
    match (typ, value) {
        (AbiType::Field, AbiValue::Field(field)) => {
            out.push(*field);
            Ok(())
        }
        (AbiType::Boolean, AbiValue::Boolean(boolean)) => {
            out.push(if *boolean { Fr::one() } else { Fr::zero() });
            Ok(())
        }
        (AbiType::Integer { sign, width }, AbiValue::Integer(integer)) => {
            if sign == "signed" && *integer < 0 {
                let encoded = if *width >= 128 {
                    *integer as u128
                } else {
                    (*integer + (1i128 << *width)) as u128
                };
                let bytes = encoded.to_be_bytes();
                let mut padded = [0u8; 32];
                padded[16..].copy_from_slice(&bytes);
                out.push(Fr::from(padded));
            } else {
                let bytes = (*integer as u128).to_be_bytes();
                let mut padded = [0u8; 32];
                padded[16..].copy_from_slice(&bytes);
                out.push(Fr::from(padded));
            }
            Ok(())
        }
        // Allow passing a raw field element for integer parameters. This is
        // needed when the value exceeds `i128::MAX` (e.g. U128 values ≥ 2^127
        // or intentional overflow values like 2^128 for testing).
        (AbiType::Integer { .. }, AbiValue::Field(field)) => {
            out.push(*field);
            Ok(())
        }
        (AbiType::Array { element, length }, AbiValue::Array(items)) => {
            if items.len() != *length {
                return Err(Error::Abi(format!(
                    "expected array of length {}, got {}",
                    length,
                    items.len()
                )));
            }
            for item in items {
                encode_value(element, item, out)?;
            }
            Ok(())
        }
        (AbiType::String { length }, AbiValue::String(string)) => {
            let bytes = string.as_bytes();
            if bytes.len() > *length {
                return Err(Error::Abi(format!(
                    "string exceeds fixed ABI length {}",
                    length
                )));
            }
            for byte in bytes {
                out.push(Fr::from(u64::from(*byte)));
            }
            for _ in bytes.len()..*length {
                out.push(Fr::zero());
            }
            Ok(())
        }
        (AbiType::Struct { fields, .. }, _) => {
            // Special struct handling
            if is_address_struct(typ) {
                match value {
                    AbiValue::Field(f) => {
                        out.push(*f);
                        Ok(())
                    }
                    AbiValue::Struct(map) => {
                        let f =
                            map.get("inner")
                                .or_else(|| map.get("address"))
                                .ok_or_else(|| {
                                    Error::Abi(
                                        "address struct must have 'inner' or 'address' field"
                                            .into(),
                                    )
                                })?;
                        encode_value(&AbiType::Field, f, out)
                    }
                    _ => Err(Error::Abi(
                        "expected Field or Struct for address type".into(),
                    )),
                }
            } else if is_function_selector_struct(typ) {
                match value {
                    AbiValue::Integer(v) => {
                        out.push(Fr::from(*v as u64));
                        Ok(())
                    }
                    AbiValue::Field(f) => {
                        out.push(*f);
                        Ok(())
                    }
                    AbiValue::Struct(map) => {
                        let inner_value = map
                            .get("value")
                            .or_else(|| map.get("inner"))
                            .ok_or_else(|| {
                                Error::Abi(
                                    "FunctionSelector struct must have 'value' or 'inner' field"
                                        .into(),
                                )
                            })?;
                        // Inner can be either Field or Integer (u32) — accept both.
                        match inner_value {
                            AbiValue::Field(f) => {
                                out.push(*f);
                                Ok(())
                            }
                            AbiValue::Integer(v) => {
                                out.push(Fr::from(*v as u64));
                                Ok(())
                            }
                            _ => Err(Error::Abi(
                                "FunctionSelector inner must be Field or Integer".into(),
                            )),
                        }
                    }
                    _ => Err(Error::Abi(
                        "expected Integer, Field, or Struct for FunctionSelector".into(),
                    )),
                }
            } else if is_wrapped_field_struct(typ) {
                match value {
                    AbiValue::Field(f) => {
                        out.push(*f);
                        Ok(())
                    }
                    AbiValue::Struct(map) => {
                        let f = map.get("inner").ok_or_else(|| {
                            Error::Abi("wrapped field struct must have 'inner' field".into())
                        })?;
                        encode_value(&AbiType::Field, f, out)
                    }
                    _ => Err(Error::Abi(
                        "expected Field or Struct for wrapped field type".into(),
                    )),
                }
            } else if is_bounded_vec_struct(typ) {
                let items = match value {
                    AbiValue::Array(items) => items,
                    AbiValue::Struct(map) => {
                        if let Some(AbiValue::Array(items)) = map.get("storage") {
                            items
                        } else {
                            return Err(Error::Abi(
                                "BoundedVec struct must have 'storage' array field".into(),
                            ));
                        }
                    }
                    _ => return Err(Error::Abi("expected Array or Struct for BoundedVec".into())),
                };
                let (elem_type, max_len) = match &fields[0].typ {
                    AbiType::Array { element, length } => (element.as_ref(), *length),
                    _ => return Err(Error::Abi("BoundedVec storage must be an Array".into())),
                };
                if items.len() > max_len {
                    return Err(Error::Abi(format!(
                        "BoundedVec has {} items but max capacity is {}",
                        items.len(),
                        max_len
                    )));
                }
                for item in items {
                    encode_value(elem_type, item, out)?;
                }
                let elem_size = abi_type_size(elem_type);
                for _ in 0..(max_len - items.len()) * elem_size {
                    out.push(Fr::zero());
                }
                out.push(Fr::from(items.len() as u64));
                Ok(())
            } else if is_option_struct(typ) {
                match value {
                    AbiValue::Struct(map) => {
                        let is_some = map.get("_is_some");
                        let val = map.get("_value");
                        match (is_some, val) {
                            (Some(is_some_val), Some(value_val)) => {
                                encode_value(&fields[0].typ, is_some_val, out)?;
                                encode_value(&fields[1].typ, value_val, out)?;
                                Ok(())
                            }
                            _ => Err(Error::Abi(
                                "Option struct must have '_is_some' and '_value' fields".into(),
                            )),
                        }
                    }
                    _ => {
                        // Treat as Some(value)
                        out.push(Fr::one()); // _is_some = true
                        encode_value(&fields[1].typ, value, out)?;
                        Ok(())
                    }
                }
            } else if let AbiValue::Struct(values) = value {
                // Generic struct encoding
                for field in fields {
                    let field_value = values.get(&field.name).ok_or_else(|| {
                        Error::Abi(format!("missing struct field '{}'", field.name))
                    })?;
                    encode_value(&field.typ, field_value, out)?;
                }
                Ok(())
            } else {
                Err(Error::Abi("argument type/value mismatch".to_owned()))
            }
        }
        (AbiType::Tuple { elements }, AbiValue::Tuple(values)) => {
            if elements.len() != values.len() {
                return Err(Error::Abi(format!(
                    "expected tuple of length {}, got {}",
                    elements.len(),
                    values.len()
                )));
            }
            for (element, value) in elements.iter().zip(values) {
                encode_value(element, value, out)?;
            }
            Ok(())
        }
        _ => Err(Error::Abi("argument type/value mismatch".to_owned())),
    }
}
