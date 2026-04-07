mod buffer;
mod checkers;
mod decoder;
mod encoder;
mod selectors;
mod types;

// Re-export everything for the public API
pub use buffer::{buffer_as_fields, buffer_from_fields};
pub use checkers::{
    abi_type_size, count_arguments_size, is_address_struct, is_aztec_address_struct,
    is_bounded_vec_struct, is_eth_address_struct, is_function_selector_struct, is_option_struct,
    is_public_keys_struct, is_wrapped_field_struct,
};
pub use decoder::{decode_from_abi, AbiDecoded};
pub use encoder::{encode_arguments, encode_value};
pub use selectors::{AuthorizationSelector, EventSelector, FunctionSelector, NoteSelector};
pub use types::{
    abi_type_signature, AbiParameter, AbiType, AbiValue, ContractArtifact, FunctionArtifact,
    FunctionType,
};

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::types::Fr;

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
    fn authorization_selector_hex_roundtrip() {
        let selector = AuthorizationSelector::from_hex("0x01020304").expect("valid hex");
        assert_eq!(selector.0, [0x01, 0x02, 0x03, 0x04]);
        assert_eq!(selector.to_string(), "0x01020304");

        let json = serde_json::to_string(&selector).expect("serialize selector");
        let decoded: AuthorizationSelector =
            serde_json::from_str(&json).expect("deserialize selector");
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

    #[test]
    fn from_signature_is_deterministic() {
        let a = FunctionSelector::from_signature("sponsor_unconditionally()");
        let b = FunctionSelector::from_signature("sponsor_unconditionally()");
        assert_eq!(a, b);
    }

    #[test]
    fn from_signature_different_inputs_differ() {
        let a = FunctionSelector::from_signature("sponsor_unconditionally()");
        let b = FunctionSelector::from_signature("claim_and_end_setup((Field),u128,Field,Field)");
        assert_ne!(a, b);
    }

    #[test]
    fn from_signature_empty_string() {
        let a = FunctionSelector::from_signature("");
        let b = FunctionSelector::from_signature("");
        assert_eq!(a, b);
    }

    #[test]
    fn from_signature_produces_4_bytes() {
        let selector = FunctionSelector::from_signature("transfer(Field,Field,u64)");
        assert_eq!(selector.0.len(), 4);
    }

    #[test]
    fn function_selector_roundtrips_through_field() {
        let selector = FunctionSelector::from_signature("set_authorized(Field,bool)");
        assert_eq!(FunctionSelector::from_field(selector.to_field()), selector);
    }

    #[test]
    fn authorization_selector_roundtrips_through_field() {
        let selector =
            AuthorizationSelector::from_signature("CallAuthorization((Field),(u32),Field)");
        assert_eq!(
            AuthorizationSelector::from_field(selector.to_field()),
            selector
        );
    }

    // -- Substep 6.1: Type checkers --

    #[test]
    fn type_checker_aztec_address() {
        let typ = AbiType::Struct {
            name: "aztec::protocol_types::address::AztecAddress".to_owned(),
            fields: vec![AbiParameter {
                name: "inner".to_owned(),
                typ: AbiType::Field,
                visibility: None,
            }],
        };
        assert!(is_aztec_address_struct(&typ));
        assert!(is_address_struct(&typ));
        assert!(!is_eth_address_struct(&typ));
    }

    #[test]
    fn type_checker_eth_address() {
        let typ = AbiType::Struct {
            name: "protocol_types::address::EthAddress".to_owned(),
            fields: vec![AbiParameter {
                name: "inner".to_owned(),
                typ: AbiType::Field,
                visibility: None,
            }],
        };
        assert!(is_eth_address_struct(&typ));
        assert!(is_address_struct(&typ));
        assert!(!is_aztec_address_struct(&typ));
    }

    #[test]
    fn type_checker_function_selector() {
        let typ = AbiType::Struct {
            name: "types::abis::function_selector::FunctionSelector".to_owned(),
            fields: vec![AbiParameter {
                name: "inner".to_owned(),
                typ: AbiType::Field,
                visibility: None,
            }],
        };
        assert!(is_function_selector_struct(&typ));
    }

    #[test]
    fn type_checker_wrapped_field() {
        let yes = AbiType::Struct {
            name: "SomeWrapped".to_owned(),
            fields: vec![AbiParameter {
                name: "inner".to_owned(),
                typ: AbiType::Field,
                visibility: None,
            }],
        };
        assert!(is_wrapped_field_struct(&yes));

        let no = AbiType::Struct {
            name: "NotWrapped".to_owned(),
            fields: vec![AbiParameter {
                name: "value".to_owned(),
                typ: AbiType::Field,
                visibility: None,
            }],
        };
        assert!(!is_wrapped_field_struct(&no));
    }

    #[test]
    fn type_checker_bounded_vec() {
        let typ = AbiType::Struct {
            name: "std::collections::bounded_vec::BoundedVec".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "storage".to_owned(),
                    typ: AbiType::Array {
                        element: Box::new(AbiType::Field),
                        length: 10,
                    },
                    visibility: None,
                },
                AbiParameter {
                    name: "len".to_owned(),
                    typ: AbiType::Integer {
                        sign: "unsigned".to_owned(),
                        width: 64,
                    },
                    visibility: None,
                },
            ],
        };
        assert!(is_bounded_vec_struct(&typ));
    }

    #[test]
    fn type_checker_option() {
        let typ = AbiType::Struct {
            name: "std::option::Option".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "_is_some".to_owned(),
                    typ: AbiType::Boolean,
                    visibility: None,
                },
                AbiParameter {
                    name: "_value".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                },
            ],
        };
        assert!(is_option_struct(&typ));
    }

    #[test]
    fn type_checker_non_matching() {
        let typ = AbiType::Field;
        assert!(!is_address_struct(&typ));
        assert!(!is_function_selector_struct(&typ));
        assert!(!is_wrapped_field_struct(&typ));
        assert!(!is_bounded_vec_struct(&typ));
        assert!(!is_option_struct(&typ));
        assert!(!is_public_keys_struct(&typ));
    }

    // -- Substep 6.2: type_size --

    #[test]
    fn type_size_scalars() {
        assert_eq!(abi_type_size(&AbiType::Field), 1);
        assert_eq!(abi_type_size(&AbiType::Boolean), 1);
        assert_eq!(
            abi_type_size(&AbiType::Integer {
                sign: "unsigned".to_owned(),
                width: 64
            }),
            1
        );
    }

    #[test]
    fn type_size_string() {
        assert_eq!(abi_type_size(&AbiType::String { length: 31 }), 31);
    }

    #[test]
    fn type_size_array() {
        assert_eq!(
            abi_type_size(&AbiType::Array {
                element: Box::new(AbiType::Field),
                length: 5
            }),
            5
        );
    }

    #[test]
    fn type_size_nested_struct() {
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
        assert_eq!(abi_type_size(&typ), 2);
    }

    #[test]
    fn count_arguments_size_works() {
        let func = FunctionArtifact {
            name: "test".to_owned(),
            function_type: FunctionType::Public,
            is_initializer: false,
            is_static: false,
            is_only_self: None,
            parameters: vec![
                AbiParameter {
                    name: "a".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                },
                AbiParameter {
                    name: "b".to_owned(),
                    typ: AbiType::Array {
                        element: Box::new(AbiType::Field),
                        length: 3,
                    },
                    visibility: None,
                },
            ],
            return_types: vec![],
            error_types: None,
            selector: None,
            bytecode: None,
            verification_key_hash: None,
            verification_key: None,
            custom_attributes: None,
            is_unconstrained: None,
            debug_symbols: None,
        };
        assert_eq!(count_arguments_size(&func), 4);
    }

    // -- Substep 6.3: Enhanced encoder --

    #[test]
    fn encode_aztec_address_as_field() {
        let typ = AbiType::Struct {
            name: "aztec::protocol_types::address::AztecAddress".to_owned(),
            fields: vec![AbiParameter {
                name: "inner".to_owned(),
                typ: AbiType::Field,
                visibility: None,
            }],
        };
        let value = AbiValue::Field(Fr::from(42u64));
        let mut out = Vec::new();
        encode_value(&typ, &value, &mut out).expect("encode address");
        assert_eq!(out, vec![Fr::from(42u64)]);
    }

    #[test]
    fn encode_bounded_vec() {
        let typ = AbiType::Struct {
            name: "std::collections::bounded_vec::BoundedVec".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "storage".to_owned(),
                    typ: AbiType::Array {
                        element: Box::new(AbiType::Field),
                        length: 5,
                    },
                    visibility: None,
                },
                AbiParameter {
                    name: "len".to_owned(),
                    typ: AbiType::Integer {
                        sign: "unsigned".to_owned(),
                        width: 64,
                    },
                    visibility: None,
                },
            ],
        };
        let value = AbiValue::Array(vec![
            AbiValue::Field(Fr::from(1u64)),
            AbiValue::Field(Fr::from(2u64)),
            AbiValue::Field(Fr::from(3u64)),
        ]);
        let mut out = Vec::new();
        encode_value(&typ, &value, &mut out).expect("encode bounded vec");
        assert_eq!(out.len(), 6);
        assert_eq!(out[0], Fr::from(1u64));
        assert_eq!(out[1], Fr::from(2u64));
        assert_eq!(out[2], Fr::from(3u64));
        assert_eq!(out[3], Fr::zero());
        assert_eq!(out[4], Fr::zero());
        assert_eq!(out[5], Fr::from(3u64));
    }

    #[test]
    fn encode_option_some() {
        let typ = AbiType::Struct {
            name: "std::option::Option".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "_is_some".to_owned(),
                    typ: AbiType::Boolean,
                    visibility: None,
                },
                AbiParameter {
                    name: "_value".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                },
            ],
        };
        let value = AbiValue::Struct({
            let mut m = BTreeMap::new();
            m.insert("_is_some".to_owned(), AbiValue::Boolean(true));
            m.insert("_value".to_owned(), AbiValue::Field(Fr::from(99u64)));
            m
        });
        let mut out = Vec::new();
        encode_value(&typ, &value, &mut out).expect("encode option some");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], Fr::one());
        assert_eq!(out[1], Fr::from(99u64));
    }

    #[test]
    fn encode_option_none() {
        let typ = AbiType::Struct {
            name: "std::option::Option".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "_is_some".to_owned(),
                    typ: AbiType::Boolean,
                    visibility: None,
                },
                AbiParameter {
                    name: "_value".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                },
            ],
        };
        let value = AbiValue::Struct({
            let mut m = BTreeMap::new();
            m.insert("_is_some".to_owned(), AbiValue::Boolean(false));
            m.insert("_value".to_owned(), AbiValue::Field(Fr::zero()));
            m
        });
        let mut out = Vec::new();
        encode_value(&typ, &value, &mut out).expect("encode option none");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], Fr::zero());
        assert_eq!(out[1], Fr::zero());
    }

    #[test]
    fn encode_signed_negative_integer() {
        let typ = AbiType::Integer {
            sign: "signed".to_owned(),
            width: 8,
        };
        let value = AbiValue::Integer(-1);
        let mut out = Vec::new();
        encode_value(&typ, &value, &mut out).expect("encode signed int");
        assert_eq!(out.len(), 1);
        let bytes = out[0].to_be_bytes();
        let raw = u128::from_be_bytes(bytes[16..].try_into().unwrap());
        assert_eq!(raw, 255);
    }

    #[test]
    fn encode_signed_positive_integer() {
        let typ = AbiType::Integer {
            sign: "signed".to_owned(),
            width: 8,
        };
        let value = AbiValue::Integer(42);
        let mut out = Vec::new();
        encode_value(&typ, &value, &mut out).expect("encode signed int");
        let bytes = out[0].to_be_bytes();
        let raw = u128::from_be_bytes(bytes[16..].try_into().unwrap());
        assert_eq!(raw, 42);
    }

    // -- Substep 6.4: Decoder --

    #[test]
    fn decode_single_field() {
        let fields = vec![Fr::from(42u64)];
        let result = decode_from_abi(&[AbiType::Field], &fields).expect("decode");
        assert_eq!(result, AbiDecoded::Field(Fr::from(42u64)));
    }

    #[test]
    fn decode_boolean() {
        let fields = vec![Fr::one()];
        let result = decode_from_abi(&[AbiType::Boolean], &fields).expect("decode");
        assert_eq!(result, AbiDecoded::Boolean(true));

        let fields = vec![Fr::zero()];
        let result = decode_from_abi(&[AbiType::Boolean], &fields).expect("decode");
        assert_eq!(result, AbiDecoded::Boolean(false));
    }

    #[test]
    fn decode_unsigned_integer() {
        let typ = AbiType::Integer {
            sign: "unsigned".to_owned(),
            width: 64,
        };
        let mut encoded = Vec::new();
        encode_value(&typ, &AbiValue::Integer(42), &mut encoded).expect("encode");
        let result = decode_from_abi(&[typ], &encoded).expect("decode");
        assert_eq!(result, AbiDecoded::Integer(42));
    }

    #[test]
    fn decode_signed_negative_integer() {
        let typ = AbiType::Integer {
            sign: "signed".to_owned(),
            width: 32,
        };
        let mut encoded = Vec::new();
        encode_value(&typ, &AbiValue::Integer(-100), &mut encoded).expect("encode");
        let result = decode_from_abi(&[typ], &encoded).expect("decode");
        assert_eq!(result, AbiDecoded::Integer(-100));
    }

    #[test]
    fn decode_array() {
        let typ = AbiType::Array {
            element: Box::new(AbiType::Field),
            length: 3,
        };
        let fields = vec![Fr::from(1u64), Fr::from(2u64), Fr::from(3u64)];
        let result = decode_from_abi(&[typ], &fields).expect("decode");
        assert_eq!(
            result,
            AbiDecoded::Array(vec![
                AbiDecoded::Field(Fr::from(1u64)),
                AbiDecoded::Field(Fr::from(2u64)),
                AbiDecoded::Field(Fr::from(3u64)),
            ])
        );
    }

    #[test]
    fn decode_struct() {
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
        let fields = vec![Fr::from(10u64), Fr::from(20u64)];
        let result = decode_from_abi(&[typ], &fields).expect("decode");
        let mut expected = BTreeMap::new();
        expected.insert("x".to_owned(), AbiDecoded::Field(Fr::from(10u64)));
        expected.insert("y".to_owned(), AbiDecoded::Field(Fr::from(20u64)));
        assert_eq!(result, AbiDecoded::Struct(expected));
    }

    #[test]
    fn decode_aztec_address() {
        let typ = AbiType::Struct {
            name: "aztec::protocol_types::address::AztecAddress".to_owned(),
            fields: vec![AbiParameter {
                name: "inner".to_owned(),
                typ: AbiType::Field,
                visibility: None,
            }],
        };
        let fields = vec![Fr::from(42u64)];
        let result = decode_from_abi(&[typ], &fields).expect("decode");
        assert_eq!(
            result,
            AbiDecoded::Address(crate::types::AztecAddress(Fr::from(42u64)))
        );
    }

    #[test]
    fn decode_option_some() {
        let typ = AbiType::Struct {
            name: "std::option::Option".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "_is_some".to_owned(),
                    typ: AbiType::Boolean,
                    visibility: None,
                },
                AbiParameter {
                    name: "_value".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                },
            ],
        };
        let fields = vec![Fr::one(), Fr::from(99u64)];
        let result = decode_from_abi(&[typ], &fields).expect("decode");
        assert_eq!(result, AbiDecoded::Field(Fr::from(99u64)));
    }

    #[test]
    fn decode_option_none() {
        let typ = AbiType::Struct {
            name: "std::option::Option".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "_is_some".to_owned(),
                    typ: AbiType::Boolean,
                    visibility: None,
                },
                AbiParameter {
                    name: "_value".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                },
            ],
        };
        let fields = vec![Fr::zero(), Fr::zero()];
        let result = decode_from_abi(&[typ], &fields).expect("decode");
        assert_eq!(result, AbiDecoded::None);
    }

    #[test]
    fn decode_string() {
        let typ = AbiType::String { length: 5 };
        let fields = vec![
            Fr::from(b'H' as u64),
            Fr::from(b'e' as u64),
            Fr::from(b'l' as u64),
            Fr::from(b'l' as u64),
            Fr::from(b'o' as u64),
        ];
        let result = decode_from_abi(&[typ], &fields).expect("decode");
        assert_eq!(result, AbiDecoded::String("Hello".to_owned()));
    }

    #[test]
    fn decode_insufficient_fields_errors() {
        let typ = AbiType::Array {
            element: Box::new(AbiType::Field),
            length: 5,
        };
        let fields = vec![Fr::from(1u64), Fr::from(2u64)];
        let result = decode_from_abi(&[typ], &fields);
        assert!(result.is_err());
    }

    #[test]
    fn encode_decode_roundtrip_complex() {
        let bv_typ = AbiType::Struct {
            name: "std::collections::bounded_vec::BoundedVec".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "storage".to_owned(),
                    typ: AbiType::Array {
                        element: Box::new(AbiType::Field),
                        length: 5,
                    },
                    visibility: None,
                },
                AbiParameter {
                    name: "len".to_owned(),
                    typ: AbiType::Integer {
                        sign: "unsigned".to_owned(),
                        width: 64,
                    },
                    visibility: None,
                },
            ],
        };
        let value = AbiValue::Array(vec![
            AbiValue::Field(Fr::from(10u64)),
            AbiValue::Field(Fr::from(20u64)),
            AbiValue::Field(Fr::from(30u64)),
        ]);
        let mut encoded = Vec::new();
        encode_value(&bv_typ, &value, &mut encoded).expect("encode");
        let decoded = decode_from_abi(&[bv_typ], &encoded).expect("decode");
        assert_eq!(
            decoded,
            AbiDecoded::Array(vec![
                AbiDecoded::Field(Fr::from(10u64)),
                AbiDecoded::Field(Fr::from(20u64)),
                AbiDecoded::Field(Fr::from(30u64)),
            ])
        );
    }

    // -- Substep 6.5: NoteSelector --

    #[test]
    fn note_selector_valid() {
        let ns = NoteSelector::new(0).expect("valid");
        assert_eq!(ns.0, 0);
        let ns = NoteSelector::new(127).expect("valid");
        assert_eq!(ns.0, 127);
    }

    #[test]
    fn note_selector_rejects_128() {
        assert!(NoteSelector::new(128).is_err());
        assert!(NoteSelector::new(255).is_err());
    }

    #[test]
    fn note_selector_field_roundtrip() {
        let ns = NoteSelector::new(42).expect("valid");
        let field = ns.to_field();
        let ns2 = NoteSelector::from_field(field).expect("from_field");
        assert_eq!(ns, ns2);
    }

    #[test]
    fn note_selector_hex_roundtrip() {
        let ns = NoteSelector::from_hex("0x1a").expect("valid");
        assert_eq!(ns.0, 0x1a);
        assert_eq!(ns.to_string(), "0x1a");
    }

    #[test]
    fn note_selector_serde_roundtrip() {
        let ns = NoteSelector::new(42).expect("valid");
        let json = serde_json::to_string(&ns).expect("serialize");
        assert_eq!(json, "42");
        let decoded: NoteSelector = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, ns);
    }

    #[test]
    fn note_selector_serde_rejects_invalid() {
        let result: Result<NoteSelector, _> = serde_json::from_str("200");
        assert!(result.is_err());
    }

    // -- Substep 6.7: Contract artifact serialization --

    #[test]
    fn artifact_to_buffer_from_buffer_roundtrip() {
        let artifact = ContractArtifact::from_json(MINIMAL_ARTIFACT).expect("parse artifact");
        let buffer = artifact.to_buffer().expect("to_buffer");
        let decoded = ContractArtifact::from_buffer(&buffer).expect("from_buffer");
        assert_eq!(decoded, artifact);
    }

    #[test]
    fn artifact_to_json_from_json_roundtrip() {
        let artifact = ContractArtifact::from_json(MINIMAL_ARTIFACT).expect("parse artifact");
        let json = artifact.to_json().expect("to_json");
        let decoded = ContractArtifact::from_json(&json).expect("from_json");
        assert_eq!(decoded, artifact);
    }

    #[test]
    fn artifact_from_buffer_rejects_invalid() {
        let result = ContractArtifact::from_buffer(b"not json");
        assert!(result.is_err());
    }

    // -- Substep 6.8: Buffer <-> Fields --

    #[test]
    fn buffer_fields_roundtrip() {
        let data = b"Hello, Aztec! This is a test of buffer encoding.";
        let fields = buffer_as_fields(data, 100).expect("encode");
        let decoded = buffer_from_fields(&fields).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn buffer_fields_empty() {
        let fields = buffer_as_fields(&[], 10).expect("encode");
        assert_eq!(fields.len(), 10);
        assert_eq!(fields[0], Fr::from(0u64));
        let decoded = buffer_from_fields(&fields).expect("decode");
        assert!(decoded.is_empty());
    }

    #[test]
    fn buffer_fields_exactly_31_bytes() {
        let data = [0xABu8; 31];
        let fields = buffer_as_fields(&data, 10).expect("encode");
        assert_eq!(fields[0], Fr::from(31u64));
        let decoded = buffer_from_fields(&fields).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn buffer_fields_62_bytes() {
        let data = [0xFFu8; 62];
        let fields = buffer_as_fields(&data, 10).expect("encode");
        assert_eq!(fields[0], Fr::from(62u64));
        let decoded = buffer_from_fields(&fields).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn buffer_fields_exceeds_max() {
        let data = [0u8; 100];
        let result = buffer_as_fields(&data, 3);
        assert!(result.is_err());
    }

    #[test]
    fn buffer_from_fields_empty_errors() {
        let result = buffer_from_fields(&[]);
        assert!(result.is_err());
    }
}
