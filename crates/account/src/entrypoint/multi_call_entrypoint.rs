//! Default multi-call entrypoint for unsigned transactions.
//!
//! Port of TS `yarn-project/entrypoints/src/default_multi_call_entrypoint.ts`.

use std::collections::BTreeMap;

use aztec_core::abi::{
    encode_arguments, AbiParameter, AbiType, AbiValue, FunctionArtifact, FunctionSelector,
    FunctionType,
};
use aztec_core::constants::protocol_contract_address;
use aztec_core::fee::GasSettings;
use aztec_core::hash::ChainInfo;
use aztec_core::tx::{ExecutionPayload, FunctionCall, HashedValues, TxContext};
use aztec_core::types::{AztecAddress, Fr};
use aztec_core::Error;

use crate::account::TxExecutionRequest;

use super::encoding::EncodedAppEntrypointCalls;

/// Multi-call entrypoint for unsigned transactions.
pub struct DefaultMultiCallEntrypoint {
    address: AztecAddress,
}

impl DefaultMultiCallEntrypoint {
    /// Create a new multi-call entrypoint using the protocol default address.
    pub fn new() -> Self {
        Self {
            address: protocol_contract_address::multi_call_entrypoint(),
        }
    }

    /// Create a multi-call entrypoint with a custom address.
    pub fn with_address(address: AztecAddress) -> Self {
        Self { address }
    }

    /// Create a full transaction execution request.
    pub fn create_tx_execution_request(
        &self,
        exec: ExecutionPayload,
        gas_settings: GasSettings,
        chain_info: &ChainInfo,
    ) -> Result<TxExecutionRequest, Error> {
        let call_data = self.build_entrypoint_call_data(&exec)?;
        let entrypoint_hashed_args = HashedValues::from_args(call_data.encoded_args.clone());

        let mut args_of_calls = call_data.encoded_calls.hashed_args().to_vec();
        args_of_calls.push(entrypoint_hashed_args.clone());
        args_of_calls.extend(exec.extra_hashed_args);

        Ok(TxExecutionRequest {
            origin: self.address,
            function_selector: call_data.function_selector,
            first_call_args_hash: entrypoint_hashed_args.hash,
            tx_context: TxContext {
                chain_id: chain_info.chain_id,
                version: chain_info.version,
                gas_settings,
            },
            args_of_calls,
            auth_witnesses: exec.auth_witnesses,
            capsules: exec.capsules,
            salt: Fr::random(),
            fee_payer: exec.fee_payer.filter(|fp| *fp != self.address),
        })
    }

    /// Wrap an `ExecutionPayload` through the multi-call entrypoint.
    pub fn wrap_execution_payload(
        &self,
        exec: ExecutionPayload,
    ) -> Result<ExecutionPayload, Error> {
        let call_data = self.build_entrypoint_call_data(&exec)?;

        let entrypoint_call = FunctionCall {
            to: self.address,
            selector: call_data.function_selector,
            args: vec![build_app_payload_value(&call_data.encoded_calls)],
            function_type: FunctionType::Private,
            is_static: false,
            hide_msg_sender: false,
        };

        let mut extra_hashed_args = call_data.encoded_calls.hashed_args().to_vec();
        extra_hashed_args.extend(exec.extra_hashed_args);

        Ok(ExecutionPayload {
            calls: vec![entrypoint_call],
            auth_witnesses: exec.auth_witnesses,
            capsules: exec.capsules,
            extra_hashed_args,
            fee_payer: exec.fee_payer,
        })
    }

    fn build_entrypoint_call_data(
        &self,
        exec: &ExecutionPayload,
    ) -> Result<EntrypointCallData, Error> {
        let encoded_calls = EncodedAppEntrypointCalls::create(&exec.calls, None)?;
        let abi = Self::entrypoint_abi();
        let encoded_args = encode_arguments(&abi, &[build_app_payload_value(&encoded_calls)])?;
        let function_selector =
            FunctionSelector::from_name_and_parameters(&abi.name, &abi.parameters);

        Ok(EntrypointCallData {
            encoded_calls,
            encoded_args,
            function_selector,
        })
    }

    fn entrypoint_abi() -> FunctionArtifact {
        let function_selector_struct = AbiType::Struct {
            name: "authwit::aztec::protocol_types::abis::function_selector::FunctionSelector"
                .to_owned(),
            fields: vec![AbiParameter {
                name: "inner".to_owned(),
                typ: AbiType::Integer {
                    sign: "unsigned".to_owned(),
                    width: 32,
                },
                visibility: None,
            }],
        };
        let address_struct = AbiType::Struct {
            name: "authwit::aztec::protocol_types::address::AztecAddress".to_owned(),
            fields: vec![AbiParameter {
                name: "inner".to_owned(),
                typ: AbiType::Field,
                visibility: None,
            }],
        };
        let function_call_struct = AbiType::Struct {
            name: "authwit::entrypoint::function_call::FunctionCall".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "args_hash".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                },
                AbiParameter {
                    name: "function_selector".to_owned(),
                    typ: function_selector_struct,
                    visibility: None,
                },
                AbiParameter {
                    name: "target_address".to_owned(),
                    typ: address_struct,
                    visibility: None,
                },
                AbiParameter {
                    name: "is_public".to_owned(),
                    typ: AbiType::Boolean,
                    visibility: None,
                },
                AbiParameter {
                    name: "hide_msg_sender".to_owned(),
                    typ: AbiType::Boolean,
                    visibility: None,
                },
                AbiParameter {
                    name: "is_static".to_owned(),
                    typ: AbiType::Boolean,
                    visibility: None,
                },
            ],
        };
        let app_payload_struct = AbiType::Struct {
            name: "authwit::entrypoint::app::AppPayload".to_owned(),
            fields: vec![
                AbiParameter {
                    name: "function_calls".to_owned(),
                    typ: AbiType::Array {
                        element: Box::new(function_call_struct),
                        length: 5,
                    },
                    visibility: None,
                },
                AbiParameter {
                    name: "tx_nonce".to_owned(),
                    typ: AbiType::Field,
                    visibility: None,
                },
            ],
        };

        FunctionArtifact {
            name: "entrypoint".to_owned(),
            function_type: FunctionType::Private,
            is_initializer: false,
            is_static: false,
            is_only_self: Some(false),
            parameters: vec![AbiParameter {
                name: "app_payload".to_owned(),
                typ: app_payload_struct,
                visibility: Some("public".to_owned()),
            }],
            return_types: vec![],
            error_types: Some(serde_json::Value::Object(Default::default())),
            selector: None,
            bytecode: None,
            verification_key_hash: None,
            verification_key: None,
            custom_attributes: None,
            is_unconstrained: None,
            debug_symbols: None,
        }
    }

    /// Get the address of this entrypoint.
    pub fn address(&self) -> AztecAddress {
        self.address
    }
}

impl Default for DefaultMultiCallEntrypoint {
    fn default() -> Self {
        Self::new()
    }
}

struct EntrypointCallData {
    encoded_calls: EncodedAppEntrypointCalls,
    encoded_args: Vec<Fr>,
    function_selector: FunctionSelector,
}

fn build_app_payload_value(encoded_calls: &EncodedAppEntrypointCalls) -> AbiValue {
    let mut payload = BTreeMap::new();
    payload.insert(
        "function_calls".to_owned(),
        AbiValue::Array(
            encoded_calls
                .encoded_calls()
                .iter()
                .map(|call| {
                    let mut value = BTreeMap::new();
                    value.insert("args_hash".to_owned(), AbiValue::Field(call.args_hash));
                    value.insert(
                        "function_selector".to_owned(),
                        AbiValue::Field(call.function_selector),
                    );
                    value.insert(
                        "target_address".to_owned(),
                        AbiValue::Field(call.target_address),
                    );
                    value.insert("is_public".to_owned(), AbiValue::Boolean(call.is_public));
                    value.insert(
                        "hide_msg_sender".to_owned(),
                        AbiValue::Boolean(call.hide_msg_sender),
                    );
                    value.insert("is_static".to_owned(), AbiValue::Boolean(call.is_static));
                    AbiValue::Struct(value)
                })
                .collect(),
        ),
    );
    payload.insert(
        "tx_nonce".to_owned(),
        AbiValue::Field(encoded_calls.tx_nonce()),
    );
    AbiValue::Struct(payload)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use aztec_core::abi::FunctionSelector;

    fn sample_chain_info() -> ChainInfo {
        ChainInfo {
            chain_id: Fr::from(31337u64),
            version: Fr::from(1u64),
        }
    }

    #[test]
    fn default_address_is_multi_call_entrypoint() {
        let ep = DefaultMultiCallEntrypoint::new();
        assert_eq!(
            ep.address(),
            protocol_contract_address::multi_call_entrypoint()
        );
    }

    #[test]
    fn wrap_creates_single_call_to_multi_call_address() {
        let ep = DefaultMultiCallEntrypoint::new();
        let exec = ExecutionPayload {
            calls: vec![FunctionCall {
                to: AztecAddress::from(1u64),
                selector: FunctionSelector::from_hex("0x11223344").expect("valid"),
                args: vec![AbiValue::Field(Fr::from(42u64))],
                function_type: FunctionType::Private,
                is_static: false,
                hide_msg_sender: false,
            }],
            ..Default::default()
        };

        let wrapped = ep.wrap_execution_payload(exec).expect("wrap");

        assert_eq!(wrapped.calls.len(), 1);
        assert_eq!(
            wrapped.calls[0].to,
            protocol_contract_address::multi_call_entrypoint()
        );
        assert!(wrapped.auth_witnesses.is_empty());
        assert_eq!(wrapped.extra_hashed_args.len(), 5);
    }

    #[test]
    fn create_tx_execution_request_uses_upstream_shape() {
        let ep = DefaultMultiCallEntrypoint::new();
        let exec = ExecutionPayload {
            calls: vec![FunctionCall {
                to: AztecAddress::from(1u64),
                selector: FunctionSelector::from_hex("0x11223344").expect("valid"),
                args: vec![AbiValue::Field(Fr::from(42u64))],
                function_type: FunctionType::Private,
                is_static: false,
                hide_msg_sender: false,
            }],
            ..Default::default()
        };

        let request = ep
            .create_tx_execution_request(exec, GasSettings::default(), &sample_chain_info())
            .expect("request");

        assert_eq!(
            request.origin,
            protocol_contract_address::multi_call_entrypoint()
        );
        assert_eq!(request.args_of_calls.len(), 6);
        assert_ne!(request.first_call_args_hash, Fr::zero());
    }

    #[test]
    fn custom_address() {
        let ep = DefaultMultiCallEntrypoint::with_address(AztecAddress::from(99u64));
        assert_eq!(ep.address(), AztecAddress::from(99u64));
    }
}
