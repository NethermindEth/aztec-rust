//! A simple [`AccountProvider`] that manages a single account.

use async_trait::async_trait;

use crate::account::{AccountContract, EntrypointOptions};
use crate::error::Error;
use crate::types::{AztecAddress, CompleteAddress};
use crate::wallet::{AccountProvider, Aliased, ChainInfo, MessageHashOrIntent};
use aztec_core::fee::GasSettings;
use aztec_core::tx::{AuthWitness, ExecutionPayload};
use aztec_pxe_client::TxExecutionRequest;

/// A simple [`AccountProvider`] that manages a single account.
///
/// This is the most common pattern: one secret key, one account contract,
/// one wallet. For multi-account wallets, implement [`AccountProvider`] directly.
///
/// Stores an [`AccountContract`] and creates fresh account instances on
/// demand (avoiding the trait-object clone problem).
pub struct SingleAccountProvider {
    complete_address: CompleteAddress,
    account_contract: Box<dyn AccountContract>,
    alias: String,
}

impl SingleAccountProvider {
    /// Create a new single-account provider.
    pub fn new(
        complete_address: CompleteAddress,
        account_contract: Box<dyn AccountContract>,
        alias: impl Into<String>,
    ) -> Self {
        Self {
            complete_address,
            account_contract,
            alias: alias.into(),
        }
    }
}

#[async_trait]
impl AccountProvider for SingleAccountProvider {
    async fn create_tx_execution_request(
        &self,
        from: &AztecAddress,
        exec: ExecutionPayload,
        gas_settings: GasSettings,
        chain_info: &ChainInfo,
        fee_payer: Option<AztecAddress>,
        fee_payment_method: Option<u8>,
    ) -> Result<TxExecutionRequest, Error> {
        if *from != self.complete_address.address {
            return Err(Error::InvalidData(format!("account not found: {from}")));
        }

        let account = self.account_contract.account(self.complete_address.clone());
        let options = EntrypointOptions {
            fee_payer,
            gas_settings: Some(gas_settings.clone()),
            fee_payment_method,
        };

        let tx_request = account
            .create_tx_execution_request(exec, gas_settings, chain_info, options)
            .await?;

        // Bridge from the account crate's structured TxExecutionRequest
        // to the PXE crate's opaque JSON TxExecutionRequest
        let data =
            serde_json::to_value(&tx_request).map_err(|e| Error::InvalidData(e.to_string()))?;

        Ok(TxExecutionRequest { data })
    }

    async fn create_auth_wit(
        &self,
        from: &AztecAddress,
        intent: MessageHashOrIntent,
        chain_info: &ChainInfo,
    ) -> Result<AuthWitness, Error> {
        if *from != self.complete_address.address {
            return Err(Error::InvalidData(format!("account not found: {from}")));
        }

        // Resolve intent → hash so the AuthorizationProvider always receives
        // a resolved hash to sign, keeping auth providers simple.
        let resolved = match &intent {
            MessageHashOrIntent::Hash { .. } => intent,
            MessageHashOrIntent::Intent { .. } | MessageHashOrIntent::InnerHash { .. } => {
                let hash = aztec_core::hash::compute_auth_wit_message_hash(&intent, chain_info);
                MessageHashOrIntent::Hash { hash }
            }
        };

        let account = self.account_contract.account(self.complete_address.clone());
        let mut witness = account
            .create_auth_wit(resolved.clone(), chain_info)
            .await?;

        // Set the request_hash on the witness so consumers can identify which
        // message this witness authorizes.
        if let MessageHashOrIntent::Hash { hash } = &resolved {
            witness.request_hash = *hash;
        }

        Ok(witness)
    }

    async fn get_complete_address(
        &self,
        address: &AztecAddress,
    ) -> Result<Option<CompleteAddress>, Error> {
        if *address == self.complete_address.address {
            Ok(Some(self.complete_address.clone()))
        } else {
            Ok(None)
        }
    }

    async fn get_accounts(&self) -> Result<Vec<Aliased<AztecAddress>>, Error> {
        Ok(vec![Aliased {
            alias: self.alias.clone(),
            item: self.complete_address.address,
        }])
    }
}
