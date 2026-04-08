//! Meta payment method that routes fee payment through an account entrypoint.
//!
//! Port of TS `yarn-project/aztec.js/src/wallet/account_entrypoint_meta_payment_method.ts`.

use async_trait::async_trait;
use std::sync::Arc;

use aztec_core::constants::protocol_contract_address;
use aztec_core::tx::ExecutionPayload;
use aztec_core::types::AztecAddress;
use aztec_core::Error;
use aztec_fee::FeePaymentMethod;

use crate::account::Account;
use crate::entrypoint::{AccountFeePaymentMethodOptions, DefaultAccountEntrypointOptions};

/// Wraps a `FeePaymentMethod` so that fee payment is routed through the
/// account contract's entrypoint. This enables an account to pay for
/// its own deployment transaction.
///
/// The inner payment method's execution payload gets wrapped as a call
/// to the account's `entrypoint()` function with the appropriate
/// fee payment method option.
pub struct AccountEntrypointMetaPaymentMethod {
    account: Arc<dyn Account>,
    inner: Option<Arc<dyn FeePaymentMethod>>,
    fee_entrypoint_options: Option<DefaultAccountEntrypointOptions>,
}

impl AccountEntrypointMetaPaymentMethod {
    /// Create a new meta payment method.
    ///
    /// - `account`: The account whose entrypoint will handle fee payment.
    /// - `inner`: Optional inner fee payment method. If `None`, assumes the
    ///   account pays from its existing Fee Juice balance.
    /// - `fee_entrypoint_options`: Optional explicit entrypoint options.
    ///   If `None`, auto-detects based on fee payer and inner calls.
    pub fn new(
        account: Arc<dyn Account>,
        inner: Option<Arc<dyn FeePaymentMethod>>,
        fee_entrypoint_options: Option<DefaultAccountEntrypointOptions>,
    ) -> Self {
        Self {
            account,
            inner,
            fee_entrypoint_options,
        }
    }
}

#[async_trait]
impl FeePaymentMethod for AccountEntrypointMetaPaymentMethod {
    async fn get_asset(&self) -> Result<AztecAddress, Error> {
        match &self.inner {
            Some(method) => method.get_asset().await,
            None => Ok(protocol_contract_address::fee_juice()),
        }
    }

    async fn get_fee_payer(&self) -> Result<AztecAddress, Error> {
        match &self.inner {
            Some(method) => method.get_fee_payer().await,
            None => Ok(self.account.address()),
        }
    }

    async fn get_fee_execution_payload(&self) -> Result<ExecutionPayload, Error> {
        // 1. Get inner payload (or empty).
        let inner_payload = match &self.inner {
            Some(method) => method.get_fee_execution_payload().await?,
            None => ExecutionPayload::default(),
        };

        // 2. Determine fee entrypoint options.
        let options = match self.fee_entrypoint_options.clone() {
            Some(opts) => opts,
            None => {
                let fee_payer = self.get_fee_payer().await?;
                let is_payer = fee_payer == self.account.address();
                let fee_payment_method_options = if is_payer && !inner_payload.calls.is_empty() {
                    AccountFeePaymentMethodOptions::FeeJuiceWithClaim
                } else if is_payer {
                    AccountFeePaymentMethodOptions::PreexistingFeeJuice
                } else {
                    AccountFeePaymentMethodOptions::External
                };

                DefaultAccountEntrypointOptions {
                    cancellable: false,
                    tx_nonce: None,
                    fee_payment_method_options,
                }
            }
        };

        // 3. Wrap the inner payload through the account's entrypoint.
        self.account
            .wrap_execution_payload(inner_payload, options.into())
            .await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aztec_core::tx::AuthWitness;
    use aztec_core::types::{CompleteAddress, Fr, PublicKeys};

    use crate::account::{AuthorizationProvider, EntrypointOptions};
    use crate::wallet::{ChainInfo, MessageHashOrIntent};

    struct MockPayerAccount {
        addr: CompleteAddress,
    }

    #[async_trait]
    impl AuthorizationProvider for MockPayerAccount {
        async fn create_auth_wit(
            &self,
            _intent: MessageHashOrIntent,
            _chain_info: &ChainInfo,
        ) -> Result<AuthWitness, Error> {
            Ok(AuthWitness::default())
        }
    }

    #[async_trait]
    impl Account for MockPayerAccount {
        fn complete_address(&self) -> &CompleteAddress {
            &self.addr
        }

        fn address(&self) -> AztecAddress {
            self.addr.address
        }

        async fn create_tx_execution_request(
            &self,
            _exec: ExecutionPayload,
            _gas_settings: aztec_core::fee::GasSettings,
            _chain_info: &ChainInfo,
            _options: EntrypointOptions,
        ) -> Result<crate::account::TxExecutionRequest, Error> {
            unimplemented!()
        }

        async fn wrap_execution_payload(
            &self,
            exec: ExecutionPayload,
            _options: EntrypointOptions,
        ) -> Result<ExecutionPayload, Error> {
            // Simple mock: just return the payload with fee_payer set.
            Ok(ExecutionPayload {
                fee_payer: Some(self.addr.address),
                ..exec
            })
        }
    }

    fn mock_account(addr: u64) -> Arc<dyn Account> {
        Arc::new(MockPayerAccount {
            addr: CompleteAddress {
                address: AztecAddress::from(addr),
                public_keys: PublicKeys::default(),
                partial_address: Fr::zero(),
            },
        })
    }

    struct MockInnerFeeMethod {
        payer: AztecAddress,
    }

    #[async_trait]
    impl FeePaymentMethod for MockInnerFeeMethod {
        async fn get_asset(&self) -> Result<AztecAddress, Error> {
            Ok(protocol_contract_address::fee_juice())
        }

        async fn get_fee_payer(&self) -> Result<AztecAddress, Error> {
            Ok(self.payer)
        }

        async fn get_fee_execution_payload(&self) -> Result<ExecutionPayload, Error> {
            Ok(ExecutionPayload::default())
        }
    }

    #[tokio::test]
    async fn no_inner_returns_wrapped_empty_payload() {
        let account = mock_account(42);
        let meta = AccountEntrypointMetaPaymentMethod::new(account, None, None);

        let payload = meta.get_fee_execution_payload().await.expect("payload");
        // Should have the account's address as fee payer (from mock wrap)
        assert_eq!(payload.fee_payer, Some(AztecAddress::from(42u64)));
    }

    #[tokio::test]
    async fn get_asset_delegates_to_inner() {
        let account = mock_account(42);
        let inner: Arc<dyn FeePaymentMethod> = Arc::new(MockInnerFeeMethod {
            payer: AztecAddress::from(42u64),
        });
        let meta = AccountEntrypointMetaPaymentMethod::new(account, Some(inner), None);

        let asset = meta.get_asset().await.expect("asset");
        assert_eq!(asset, protocol_contract_address::fee_juice());
    }

    #[tokio::test]
    async fn get_fee_payer_with_no_inner() {
        let account = mock_account(42);
        let meta = AccountEntrypointMetaPaymentMethod::new(account, None, None);

        let payer = meta.get_fee_payer().await.expect("payer");
        assert_eq!(payer, AztecAddress::from(42u64));
    }

    #[tokio::test]
    async fn get_fee_payer_delegates_to_inner() {
        let account = mock_account(42);
        let inner: Arc<dyn FeePaymentMethod> = Arc::new(MockInnerFeeMethod {
            payer: AztecAddress::from(99u64),
        });
        let meta = AccountEntrypointMetaPaymentMethod::new(account, Some(inner), None);

        let payer = meta.get_fee_payer().await.expect("payer");
        assert_eq!(payer, AztecAddress::from(99u64));
    }

    #[tokio::test]
    async fn explicit_options_override_auto_detection() {
        let account = mock_account(42);
        let explicit_opts = DefaultAccountEntrypointOptions {
            cancellable: true,
            tx_nonce: Some(Fr::from(7u64)),
            fee_payment_method_options: AccountFeePaymentMethodOptions::External,
        };
        let meta = AccountEntrypointMetaPaymentMethod::new(account, None, Some(explicit_opts));

        // Should not panic (auto-detection not triggered)
        let _payload = meta.get_fee_execution_payload().await.expect("payload");
    }
}
