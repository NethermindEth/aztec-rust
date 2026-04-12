//! Default account entrypoint — wraps calls through the account contract.
//!
//! Port of TS `yarn-project/entrypoints/src/account_entrypoint.ts`.

use std::collections::BTreeMap;

use aztec_core::abi::{
    encode_arguments, AbiParameter, AbiType, AbiValue, FunctionArtifact, FunctionSelector,
    FunctionType,
};
use aztec_core::fee::GasSettings;
use aztec_core::hash::ChainInfo;
use aztec_core::tx::{ExecutionPayload, FunctionCall, HashedValues, TxContext};
use aztec_core::types::{AztecAddress, Fr};
use aztec_core::Error;

use crate::account::{AuthorizationProvider, EntrypointOptions, TxExecutionRequest};
use crate::wallet::MessageHashOrIntent;

use super::encoding::EncodedAppEntrypointCalls;

/// Fee payment method options for account entrypoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountFeePaymentMethodOptions {
    /// Another contract/account pays fees.
    External = 0,
    /// Account pays from existing Fee Juice balance.
    PreexistingFeeJuice = 1,
    /// Account claims Fee Juice from L1 bridge and pays in same tx.
    FeeJuiceWithClaim = 2,
}

/// Options for `DefaultAccountEntrypoint`.
#[derive(Clone, Debug)]
pub struct DefaultAccountEntrypointOptions {
    /// Whether the transaction can be cancelled.
    pub cancellable: bool,
    /// Optional explicit tx nonce (random if None).
    pub tx_nonce: Option<Fr>,
    /// Fee payment method option.
    pub fee_payment_method_options: AccountFeePaymentMethodOptions,
}

impl Default for DefaultAccountEntrypointOptions {
    fn default() -> Self {
        Self {
            cancellable: false,
            tx_nonce: None,
            fee_payment_method_options: AccountFeePaymentMethodOptions::External,
        }
    }
}

impl From<DefaultAccountEntrypointOptions> for EntrypointOptions {
    fn from(_opts: DefaultAccountEntrypointOptions) -> Self {
        Self {
            fee_payer: None,
            gas_settings: None,
            fee_payment_method: None,
        }
    }
}

struct EntrypointCallData {
    encoded_calls: EncodedAppEntrypointCalls,
    encoded_args: Vec<Fr>,
    function_selector: FunctionSelector,
    payload_auth_witness: aztec_core::tx::AuthWitness,
}

/// Standard account entrypoint — wraps calls through the account contract.
pub struct DefaultAccountEntrypoint {
    address: AztecAddress,
    auth: Box<dyn AuthorizationProvider>,
}

impl DefaultAccountEntrypoint {
    /// Create a new account entrypoint for the given address and auth provider.
    pub fn new(address: AztecAddress, auth: Box<dyn AuthorizationProvider>) -> Self {
        Self { address, auth }
    }

    /// Create a full transaction execution request.
    pub async fn create_tx_execution_request(
        &self,
        exec: ExecutionPayload,
        gas_settings: GasSettings,
        chain_info: &ChainInfo,
        options: &DefaultAccountEntrypointOptions,
    ) -> Result<TxExecutionRequest, Error> {
        let call_data = self
            .build_entrypoint_call_data(&exec, chain_info, options)
            .await?;
        let entrypoint_hashed_args = HashedValues::from_args(call_data.encoded_args.clone());

        let mut args_of_calls = call_data.encoded_calls.hashed_args().to_vec();
        args_of_calls.push(entrypoint_hashed_args.clone());
        args_of_calls.extend(exec.extra_hashed_args);

        let mut auth_witnesses = exec.auth_witnesses;
        auth_witnesses.push(call_data.payload_auth_witness);

        let fee_payer = exec.fee_payer.filter(|fp| *fp != self.address);

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
            auth_witnesses,
            capsules: exec.capsules,
            salt: Fr::random(),
            fee_payer,
        })
    }

    /// Create a wrapped `ExecutionPayload` by encoding calls through the account entrypoint.
    pub async fn wrap_execution_payload(
        &self,
        exec: ExecutionPayload,
        chain_info: &ChainInfo,
        options: &DefaultAccountEntrypointOptions,
    ) -> Result<ExecutionPayload, Error> {
        let call_data = self
            .build_entrypoint_call_data(&exec, chain_info, options)
            .await?;

        let entrypoint_call = FunctionCall {
            to: self.address,
            selector: call_data.function_selector,
            args: vec![
                build_app_payload_value(&call_data.encoded_calls),
                AbiValue::Integer(options.fee_payment_method_options as i128),
                AbiValue::Boolean(options.cancellable),
            ],
            function_type: FunctionType::Private,
            is_static: false,
            hide_msg_sender: false,
        };

        let mut wrapped_auth_witnesses = vec![call_data.payload_auth_witness];
        wrapped_auth_witnesses.extend(exec.auth_witnesses);

        let mut wrapped_hashed_args = call_data.encoded_calls.hashed_args().to_vec();
        wrapped_hashed_args.extend(exec.extra_hashed_args);

        Ok(ExecutionPayload {
            calls: vec![entrypoint_call],
            auth_witnesses: wrapped_auth_witnesses,
            capsules: exec.capsules,
            extra_hashed_args: wrapped_hashed_args,
            fee_payer: exec.fee_payer.or(Some(self.address)),
        })
    }

    /// Return the ABI for the standard account `entrypoint` function.
    ///
    /// This is useful for account contract implementations that need to
    /// include the entrypoint in their contract artifact.
    pub fn entrypoint_abi() -> FunctionArtifact {
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
            parameters: vec![
                AbiParameter {
                    name: "app_payload".to_owned(),
                    typ: app_payload_struct,
                    visibility: Some("public".to_owned()),
                },
                AbiParameter {
                    name: "fee_payment_method".to_owned(),
                    typ: AbiType::Integer {
                        sign: "unsigned".to_owned(),
                        width: 8,
                    },
                    visibility: None,
                },
                AbiParameter {
                    name: "cancellable".to_owned(),
                    typ: AbiType::Boolean,
                    visibility: None,
                },
            ],
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

    async fn build_entrypoint_call_data(
        &self,
        exec: &ExecutionPayload,
        chain_info: &ChainInfo,
        options: &DefaultAccountEntrypointOptions,
    ) -> Result<EntrypointCallData, Error> {
        let encoded_calls = EncodedAppEntrypointCalls::create(&exec.calls, options.tx_nonce)?;
        let abi = Self::entrypoint_abi();
        let encoded_args = encode_arguments(
            &abi,
            &[
                build_app_payload_value(&encoded_calls),
                AbiValue::Integer(options.fee_payment_method_options as i128),
                AbiValue::Boolean(options.cancellable),
            ],
        )?;
        let function_selector =
            FunctionSelector::from_name_and_parameters(&abi.name, &abi.parameters);
        let payload_auth_witness = self
            .auth
            .create_auth_wit(
                MessageHashOrIntent::Hash {
                    hash: encoded_calls.hash(),
                },
                chain_info,
            )
            .await?;

        Ok(EntrypointCallData {
            encoded_calls,
            encoded_args,
            function_selector,
            payload_auth_witness,
        })
    }

    /// Get the address of this entrypoint's account.
    pub fn address(&self) -> AztecAddress {
        self.address
    }
}

fn build_app_payload_value(encoded_calls: &EncodedAppEntrypointCalls) -> AbiValue {
    let mut payload = BTreeMap::new();
    payload.insert(
        "function_calls".to_owned(),
        AbiValue::Array(
            encoded_calls
                .encoded_calls()
                .iter()
                .map(build_encoded_call_value)
                .collect(),
        ),
    );
    payload.insert(
        "tx_nonce".to_owned(),
        AbiValue::Field(encoded_calls.tx_nonce()),
    );
    AbiValue::Struct(payload)
}

fn build_encoded_call_value(call: &super::encoding::EncodedCallView) -> AbiValue {
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
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aztec_core::abi::{AbiValue, FunctionSelector, FunctionType};
    use aztec_core::tx::AuthWitness;

    struct MockAuth;

    #[async_trait]
    impl AuthorizationProvider for MockAuth {
        async fn create_auth_wit(
            &self,
            intent: MessageHashOrIntent,
            _chain_info: &ChainInfo,
        ) -> Result<AuthWitness, Error> {
            let hash = match intent {
                MessageHashOrIntent::Hash { hash } => hash,
                _ => Fr::zero(),
            };
            Ok(AuthWitness {
                request_hash: hash,
                fields: vec![hash, Fr::from(1u64)],
            })
        }
    }

    fn sample_chain_info() -> ChainInfo {
        ChainInfo {
            chain_id: Fr::from(31337u64),
            version: Fr::from(1u64),
        }
    }

    fn sample_exec() -> ExecutionPayload {
        ExecutionPayload {
            calls: vec![FunctionCall {
                to: AztecAddress::from(1u64),
                selector: FunctionSelector::from_hex("0x11223344").expect("valid"),
                args: vec![AbiValue::Field(Fr::from(99u64))],
                function_type: FunctionType::Private,
                is_static: false,
                hide_msg_sender: false,
            }],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn wrap_creates_single_entrypoint_call() {
        let entrypoint =
            DefaultAccountEntrypoint::new(AztecAddress::from(42u64), Box::new(MockAuth));
        let wrapped = entrypoint
            .wrap_execution_payload(
                sample_exec(),
                &sample_chain_info(),
                &DefaultAccountEntrypointOptions::default(),
            )
            .await
            .expect("wrap");

        assert_eq!(wrapped.calls.len(), 1);
        assert_eq!(wrapped.calls[0].to, AztecAddress::from(42u64));
        assert_eq!(wrapped.auth_witnesses.len(), 1);
        assert!(!wrapped.extra_hashed_args.is_empty());
        assert_eq!(wrapped.fee_payer, Some(AztecAddress::from(42u64)));
    }

    #[tokio::test]
    async fn wrap_prepends_payload_auth_witness() {
        let entrypoint =
            DefaultAccountEntrypoint::new(AztecAddress::from(42u64), Box::new(MockAuth));
        let exec = ExecutionPayload {
            auth_witnesses: vec![AuthWitness {
                request_hash: Fr::from(1u64),
                fields: vec![Fr::from(999u64)],
            }],
            ..sample_exec()
        };

        let wrapped = entrypoint
            .wrap_execution_payload(
                exec,
                &sample_chain_info(),
                &DefaultAccountEntrypointOptions::default(),
            )
            .await
            .expect("wrap");

        assert_eq!(wrapped.auth_witnesses.len(), 2);
        assert_ne!(wrapped.auth_witnesses[0].fields, vec![Fr::from(999u64)]);
        assert_eq!(wrapped.auth_witnesses[1].fields, vec![Fr::from(999u64)]);
    }

    #[tokio::test]
    async fn create_tx_execution_request_uses_upstream_shape() {
        let entrypoint =
            DefaultAccountEntrypoint::new(AztecAddress::from(42u64), Box::new(MockAuth));
        let gas_settings = GasSettings::default();

        let request = entrypoint
            .create_tx_execution_request(
                sample_exec(),
                gas_settings.clone(),
                &sample_chain_info(),
                &DefaultAccountEntrypointOptions::default(),
            )
            .await
            .expect("request");

        assert_eq!(request.origin, AztecAddress::from(42u64));
        assert_eq!(request.tx_context.gas_settings, gas_settings);
        assert_eq!(request.args_of_calls.len(), 6);
        assert_eq!(request.auth_witnesses.len(), 1);
        assert_ne!(request.first_call_args_hash, Fr::zero());
    }
}
