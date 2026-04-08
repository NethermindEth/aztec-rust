//! Signerless account for unsigned transactions.

use async_trait::async_trait;

use crate::account::{Account, AuthorizationProvider, EntrypointOptions};
use crate::entrypoint::DefaultMultiCallEntrypoint;
use crate::tx::{AuthWitness, ExecutionPayload};
use crate::types::{AztecAddress, CompleteAddress};
use crate::wallet::{ChainInfo, MessageHashOrIntent};

use aztec_core::fee::GasSettings;
use aztec_core::Error;

use super::account::TxExecutionRequest;

/// An account that requires no signing. Uses the `DefaultMultiCallEntrypoint`
/// to batch function calls without authorization.
///
/// Used for:
/// - Fee-sponsored transactions where the sponsor pays
/// - Protocol-level operations that don't need account identity
/// - Testing scenarios
pub struct SignerlessAccount {
    entrypoint: DefaultMultiCallEntrypoint,
}

impl SignerlessAccount {
    /// Create a new signerless account.
    pub fn new() -> Self {
        Self {
            entrypoint: DefaultMultiCallEntrypoint::new(),
        }
    }
}

impl Default for SignerlessAccount {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AuthorizationProvider for SignerlessAccount {
    async fn create_auth_wit(
        &self,
        _intent: MessageHashOrIntent,
        _chain_info: &ChainInfo,
    ) -> Result<AuthWitness, Error> {
        Err(Error::InvalidData(
            "SignerlessAccount does not support authorization witnesses".into(),
        ))
    }
}

#[async_trait]
impl Account for SignerlessAccount {
    fn complete_address(&self) -> &CompleteAddress {
        panic!("SignerlessAccount does not have a complete address")
    }

    fn address(&self) -> AztecAddress {
        panic!("SignerlessAccount does not have an address")
    }

    async fn create_tx_execution_request(
        &self,
        exec: ExecutionPayload,
        gas_settings: GasSettings,
        chain_info: &ChainInfo,
        _options: EntrypointOptions,
    ) -> Result<TxExecutionRequest, Error> {
        self.entrypoint
            .create_tx_execution_request(exec, gas_settings, chain_info)
    }

    async fn wrap_execution_payload(
        &self,
        exec: ExecutionPayload,
        _options: EntrypointOptions,
    ) -> Result<ExecutionPayload, Error> {
        self.entrypoint.wrap_execution_payload(exec)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use aztec_core::abi::{AbiValue, FunctionSelector, FunctionType};
    use aztec_core::constants::protocol_contract_address;
    use aztec_core::fee::Gas;
    use aztec_core::tx::FunctionCall;
    use aztec_core::types::Fr;

    fn sample_chain_info() -> ChainInfo {
        ChainInfo {
            chain_id: Fr::from(31337u64),
            version: Fr::from(1u64),
        }
    }

    #[tokio::test]
    async fn create_auth_wit_returns_error() {
        let account = SignerlessAccount::new();
        let chain_info = sample_chain_info();
        let result = account
            .create_auth_wit(
                MessageHashOrIntent::Hash {
                    hash: Fr::from(1u64),
                },
                &chain_info,
            )
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not support"));
    }

    #[tokio::test]
    async fn create_tx_execution_request_delegates_to_multi_call() {
        let account = SignerlessAccount::new();
        let chain_info = sample_chain_info();

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

        let gas_settings = GasSettings {
            gas_limits: Some(Gas {
                da_gas: 100,
                l2_gas: 200,
            }),
            ..GasSettings::default()
        };

        let req = account
            .create_tx_execution_request(
                exec,
                gas_settings.clone(),
                &chain_info,
                EntrypointOptions::default(),
            )
            .await
            .expect("create tx");

        assert_eq!(
            req.origin,
            protocol_contract_address::multi_call_entrypoint()
        );
        assert_eq!(req.tx_context.gas_settings, gas_settings);
        assert_eq!(req.args_of_calls.len(), 6);
    }

    #[tokio::test]
    async fn wrap_execution_payload_delegates() {
        let account = SignerlessAccount::new();
        let exec = ExecutionPayload {
            calls: vec![FunctionCall {
                to: AztecAddress::from(1u64),
                selector: FunctionSelector::from_hex("0x11223344").expect("valid"),
                args: vec![],
                function_type: FunctionType::Private,
                is_static: false,
                hide_msg_sender: false,
            }],
            ..Default::default()
        };

        let wrapped = account
            .wrap_execution_payload(exec, EntrypointOptions::default())
            .await
            .expect("wrap");

        assert_eq!(wrapped.calls.len(), 1);
        assert_eq!(
            wrapped.calls[0].to,
            protocol_contract_address::multi_call_entrypoint()
        );
    }

    #[test]
    fn is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SignerlessAccount>();
    }

    #[test]
    fn trait_object_safety() {
        fn _assert(_: Box<dyn Account>) {}
    }

    #[test]
    fn default_impl() {
        let _ = SignerlessAccount::default();
    }
}
