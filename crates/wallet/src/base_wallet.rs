//! Production wallet implementation backed by PXE and Aztec node connections.
//!
//! [`BaseWallet`] is the standard [`Wallet`] implementation for interacting with
//! a live Aztec network. It routes private operations through a [`Pxe`] backend,
//! public queries through an [`AztecNode`] backend, and delegates authentication
//! to an [`AccountProvider`].

use async_trait::async_trait;
use tokio::time::{sleep, Duration, Instant};

use crate::abi::{AbiType, ContractArtifact};
use crate::account_provider::AccountProvider;
use crate::error::Error;
use crate::node::{
    create_aztec_node_client, wait_for_tx, AztecNode, HttpNodeClient, TxValidationResult, WaitOpts,
};
use crate::pxe::{self, Pxe, RegisterContractRequest};
use crate::tx::{AuthWitness, ExecutionPayload, FunctionCall, TxHash, TxStatus};
use crate::types::{AztecAddress, ContractInstanceWithAddress, Fr};
use crate::wallet::{
    Aliased, ChainInfo, ContractClassMetadata, ContractMetadata, EventMetadataDefinition,
    ExecuteUtilityOptions, MessageHashOrIntent, PrivateEvent, PrivateEventFilter,
    PrivateEventMetadata, ProfileOptions, SendOptions, SendResult, SimulateOptions,
    TxProfileResult, TxSimulationResult, UtilityExecutionResult, Wallet,
};

/// A production [`Wallet`] backed by PXE + Aztec node connections.
///
/// Routes private-state operations (simulate, prove, events, registration)
/// through the PXE, public-state queries (chain info, contract metadata)
/// through the Aztec node, and delegates auth witness creation to the
/// account provider.
pub struct BaseWallet<P, N, A> {
    pxe: P,
    node: N,
    accounts: A,
}

impl<P: Pxe, N: AztecNode, A: AccountProvider> BaseWallet<P, N, A> {
    /// Create a new `BaseWallet` with the given PXE, node, and account provider.
    pub fn new(pxe: P, node: N, accounts: A) -> Self {
        Self {
            pxe,
            node,
            accounts,
        }
    }

    /// Get a reference to the underlying PXE client.
    pub fn pxe(&self) -> &P {
        &self.pxe
    }

    /// Get a reference to the underlying Aztec node client.
    pub fn node(&self) -> &N {
        &self.node
    }

    /// Get a reference to the account provider.
    pub fn account_provider(&self) -> &A {
        &self.accounts
    }

    /// Merge wallet-level auth witnesses and capsules into an execution payload.
    fn merge_execution_payload(
        mut exec: ExecutionPayload,
        auth_witnesses: &[AuthWitness],
        capsules: &[crate::tx::Capsule],
    ) -> ExecutionPayload {
        exec.auth_witnesses.extend_from_slice(auth_witnesses);
        exec.capsules.extend_from_slice(capsules);
        exec
    }

    /// Build scopes from a sender address and additional scopes.
    fn build_scopes(from: &AztecAddress, additional: &[AztecAddress]) -> Vec<AztecAddress> {
        let mut scopes = additional.to_vec();
        if *from != AztecAddress(Fr::zero()) && !scopes.contains(from) {
            scopes.push(*from);
        }
        scopes
    }

    async fn wait_for_submission_checkpoint(&self, tx_hash: &TxHash) -> Result<(), Error> {
        let start_block = self.node.get_block_number().await.unwrap_or(0);
        let wait_opts = WaitOpts {
            timeout: Duration::from_secs(15),
            ..WaitOpts::default()
        };

        match wait_for_tx(&self.node, tx_hash, wait_opts).await {
            Ok(_) => Ok(()),
            Err(Error::Timeout(_)) | Err(Error::InvalidData(_)) => {
                let deadline = Instant::now() + Duration::from_secs(30);
                let mut next_log = Instant::now();
                loop {
                    match self.node.get_tx_receipt(tx_hash).await {
                        Ok(receipt) if Instant::now() >= next_log => {
                            tracing::debug!(
                                tx_hash = %tx_hash,
                                status = ?receipt.status,
                                block = ?receipt.block_number,
                                error = ?receipt.error,
                                "wait_for_submission_checkpoint polling"
                            );
                            next_log = Instant::now() + Duration::from_secs(2);
                        }
                        Err(err) if Instant::now() >= next_log => {
                            tracing::debug!(
                                tx_hash = %tx_hash,
                                error = %err,
                                "wait_for_submission_checkpoint receipt error"
                            );
                            next_log = Instant::now() + Duration::from_secs(2);
                        }
                        _ => {}
                    }
                    let current_block = self.node.get_block_number().await?;
                    if current_block > start_block {
                        return Ok(());
                    }
                    if Instant::now() >= deadline {
                        return Err(Error::Timeout(
                            "transaction was submitted but did not become checkpoint-visible"
                                .into(),
                        ));
                    }
                    sleep(Duration::from_millis(500)).await;
                }
            }
            Err(err) => Err(err),
        }
    }

    async fn simulate_public_calls(
        &self,
        tx_hash: &TxHash,
        tx_json: &serde_json::Value,
    ) -> Result<(), Error> {
        let simulation = self.node.simulate_public_calls(tx_json, false).await?;
        if let Some(revert_reason) = simulation.get("revertReason") {
            if !revert_reason.is_null() {
                let debug_logs = simulation
                    .get("debugLogs")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                return Err(Error::InvalidData(format!(
                    "node public-call preflight for tx {} reverted: reason={} debugLogs={}",
                    tx_hash, revert_reason, debug_logs
                )));
            }
        }
        Ok(())
    }

    /// Errors from the simulation preflight that should be ignored.
    ///
    /// The node's `simulatePublicCalls` C++ AVM simulation can fail for reasons
    /// that don't affect actual block execution, e.g. recently-deployed contracts
    /// appearing "not deployed" because the simulation's world-state snapshot lags
    /// behind the block-builder's.
    fn should_ignore_public_preflight_error(err: &Error) -> bool {
        match err {
            Error::InvalidData(msg) => msg.contains("is not deployed"),
            _ => false,
        }
    }
}

#[async_trait]
impl<P: Pxe, N: AztecNode, A: AccountProvider> Wallet for BaseWallet<P, N, A> {
    async fn get_chain_info(&self) -> Result<ChainInfo, Error> {
        let info = self.node.get_node_info().await?;
        Ok(ChainInfo {
            chain_id: Fr::from(info.l1_chain_id),
            version: Fr::from(info.rollup_version),
        })
    }

    async fn get_accounts(&self) -> Result<Vec<Aliased<AztecAddress>>, Error> {
        self.accounts.get_accounts().await
    }

    async fn get_address_book(&self) -> Result<Vec<Aliased<AztecAddress>>, Error> {
        let senders = self.pxe.get_senders().await?;
        Ok(senders
            .into_iter()
            .map(|addr| Aliased {
                alias: String::new(),
                item: addr,
            })
            .collect())
    }

    async fn register_sender(
        &self,
        address: AztecAddress,
        _alias: Option<String>,
    ) -> Result<AztecAddress, Error> {
        self.pxe.register_sender(&address).await
    }

    async fn get_contract_metadata(
        &self,
        address: AztecAddress,
    ) -> Result<ContractMetadata, Error> {
        let instance = self.pxe.get_contract_instance(&address).await?;
        let on_chain = self.node.get_contract(&address).await?;

        let is_published = on_chain.is_some();
        let is_updated = on_chain
            .as_ref()
            .map(|contract| {
                contract.current_contract_class_id != contract.original_contract_class_id
            })
            .unwrap_or(false);
        let updated_contract_class_id = on_chain
            .as_ref()
            .and_then(|contract| is_updated.then_some(contract.current_contract_class_id));

        // Simplified: full impl needs nullifier-based initialization check (Step 5)
        Ok(ContractMetadata {
            instance,
            is_contract_initialized: is_published,
            is_contract_published: is_published,
            is_contract_updated: is_updated,
            updated_contract_class_id,
        })
    }

    async fn get_contract_class_metadata(
        &self,
        class_id: Fr,
    ) -> Result<ContractClassMetadata, Error> {
        let artifact = self.pxe.get_contract_artifact(&class_id).await?;
        let on_chain = self.node.get_contract_class(&class_id).await?;

        Ok(ContractClassMetadata {
            is_artifact_registered: artifact.is_some(),
            is_contract_class_publicly_registered: on_chain.is_some(),
        })
    }

    async fn register_contract(
        &self,
        instance: ContractInstanceWithAddress,
        artifact: Option<ContractArtifact>,
        secret_key: Option<Fr>,
    ) -> Result<ContractInstanceWithAddress, Error> {
        let existing_instance = self.pxe.get_contract_instance(&instance.address).await?;

        match (existing_instance, artifact) {
            (Some(existing), Some(artifact))
                if existing.current_contract_class_id != instance.current_contract_class_id =>
            {
                self.pxe
                    .update_contract(&instance.address, &artifact)
                    .await?;
            }
            (Some(_), Some(_)) | (Some(_), None) => {}
            (None, artifact) => {
                let artifact = match artifact {
                    Some(artifact) => artifact,
                    None => self
                        .pxe
                        .get_contract_artifact(&instance.current_contract_class_id)
                        .await?
                        .ok_or_else(|| {
                            Error::InvalidData(format!(
                                "cannot register contract at {} without an artifact; class {} is not registered in PXE",
                                instance.address, instance.current_contract_class_id
                            ))
                        })?,
                };

                self.pxe
                    .register_contract(RegisterContractRequest {
                        instance: instance.clone(),
                        artifact: Some(artifact),
                    })
                    .await?;
            }
        }

        if let Some(sk) = secret_key {
            let complete_address = self
                .accounts
                .get_complete_address(&instance.address)
                .await?
                .ok_or_else(|| {
                    Error::InvalidData(format!(
                        "cannot register account for {}: account provider does not expose its complete address",
                        instance.address
                    ))
                })?;
            self.pxe
                .register_account(&sk, &complete_address.partial_address)
                .await?;
        }

        Ok(instance)
    }

    async fn get_private_events(
        &self,
        event_metadata: &EventMetadataDefinition,
        filter: PrivateEventFilter,
    ) -> Result<Vec<PrivateEvent>, Error> {
        let pxe_filter = pxe::PrivateEventFilter {
            contract_address: filter.contract_address,
            tx_hash: filter.tx_hash,
            from_block: filter.from_block,
            to_block: filter.to_block,
            after_log: filter.after_log.map(|l| pxe::LogId {
                block_number: l.block_number,
                block_hash: pxe::BlockHash::default(),
                tx_hash: TxHash::zero(),
                tx_index: 0,
                log_index: l.log_index,
            }),
            scopes: filter.scopes,
        };

        let packed = self
            .pxe
            .get_private_events(&event_metadata.event_selector, pxe_filter)
            .await?;

        Ok(decode_private_events(packed, event_metadata))
    }

    async fn simulate_tx(
        &self,
        exec: ExecutionPayload,
        opts: SimulateOptions,
    ) -> Result<TxSimulationResult, Error> {
        let exec = Self::merge_execution_payload(exec, &opts.auth_witnesses, &opts.capsules);
        let chain_info = self.get_chain_info().await?;
        let gas_settings = opts.gas_settings.clone().unwrap_or_default();

        let tx_request = self
            .accounts
            .create_tx_execution_request(&opts.from, exec, gas_settings, &chain_info, None)
            .await?;

        let scopes = Self::build_scopes(&opts.from, &opts.additional_scopes);

        let pxe_opts = pxe::SimulateTxOpts {
            simulate_public: true,
            skip_tx_validation: opts.skip_validation,
            skip_fee_enforcement: opts.skip_fee_enforcement,
            overrides: None,
            scopes,
        };

        let result = self.pxe.simulate_tx(&tx_request, pxe_opts).await?;

        Ok(TxSimulationResult {
            return_values: result.data.clone(),
            gas_used: None, // TODO: extract from result.data (Step 9)
        })
    }

    async fn execute_utility(
        &self,
        call: FunctionCall,
        opts: ExecuteUtilityOptions,
    ) -> Result<UtilityExecutionResult, Error> {
        let pxe_opts = pxe::ExecuteUtilityOpts {
            authwits: opts.auth_witnesses,
            scopes: vec![opts.scope],
        };

        let result = self.pxe.execute_utility(&call, pxe_opts).await?;

        Ok(UtilityExecutionResult {
            result: serde_json::to_value(&result.result).unwrap_or(serde_json::Value::Null),
            stats: result.stats,
        })
    }

    async fn profile_tx(
        &self,
        exec: ExecutionPayload,
        opts: ProfileOptions,
    ) -> Result<TxProfileResult, Error> {
        let exec = Self::merge_execution_payload(exec, &opts.auth_witnesses, &opts.capsules);
        let chain_info = self.get_chain_info().await?;
        let gas_settings = opts.gas_settings.clone().unwrap_or_default();

        let tx_request = self
            .accounts
            .create_tx_execution_request(&opts.from, exec, gas_settings, &chain_info, None)
            .await?;

        let scopes = Self::build_scopes(&opts.from, &opts.additional_scopes);

        let profile_mode = match opts.profile_mode {
            Some(super::wallet::ProfileMode::ExecutionSteps) => pxe::ProfileMode::ExecutionSteps,
            Some(super::wallet::ProfileMode::Gates) => pxe::ProfileMode::Gates,
            Some(super::wallet::ProfileMode::Full) | None => pxe::ProfileMode::Full,
        };

        let pxe_opts = pxe::ProfileTxOpts {
            profile_mode,
            skip_proof_generation: opts.skip_proof_generation,
            scopes,
        };

        let result = self.pxe.profile_tx(&tx_request, pxe_opts).await?;

        Ok(TxProfileResult {
            return_values: result.data.clone(),
            gas_used: None,
            profile_data: result.data,
        })
    }

    async fn send_tx(
        &self,
        exec: ExecutionPayload,
        opts: SendOptions,
    ) -> Result<SendResult, Error> {
        let exec = Self::merge_execution_payload(exec, &opts.auth_witnesses, &opts.capsules);
        let chain_info = self.get_chain_info().await?;
        let gas_settings = opts.gas_settings.clone().unwrap_or_default();

        let tx_request = self
            .accounts
            .create_tx_execution_request(&opts.from, exec, gas_settings, &chain_info, None)
            .await?;

        let scopes = Self::build_scopes(&opts.from, &opts.additional_scopes);

        let (tx_hash, tx_json) = {
            let proven = self.pxe.prove_tx(&tx_request, scopes.clone()).await?;
            let tx_hash = proven.tx_hash.ok_or_else(|| {
                Error::InvalidData("PXE prove_tx result did not include a tx hash".into())
            })?;

            let tx = proven.to_tx();
            let tx_json = tx.to_json_value()?;
            if !tx.public_function_calldata.is_empty() {
                match self.simulate_public_calls(&tx_hash, &tx_json).await {
                    Ok(()) => {}
                    Err(err) if Self::should_ignore_public_preflight_error(&err) => {
                        tracing::debug!(
                            tx_hash = %tx_hash,
                            error = %err,
                            "ignoring public-call preflight error"
                        );
                    }
                    Err(err) => return Err(err),
                }
            }
            (tx_hash, tx_json)
        };

        match self.node.is_valid_tx(&tx_json).await? {
            TxValidationResult::Valid => {}
            TxValidationResult::Invalid { reason } => {
                return Err(Error::InvalidData(format!(
                    "node rejected tx {} during preflight validation: {}",
                    tx_hash,
                    reason.join(", ")
                )));
            }
            TxValidationResult::Skipped { reason } => {
                tracing::debug!(
                    tx_hash = %tx_hash,
                    reasons = %reason.join(", "),
                    "node skipped tx preflight validation"
                );
            }
        }
        self.node.send_tx(&tx_json).await?;
        self.wait_for_submission_checkpoint(&tx_hash).await?;

        Ok(SendResult { tx_hash })
    }

    async fn wait_for_contract(&self, address: AztecAddress) -> Result<(), Error> {
        let timeout = Duration::from_secs(30);
        let interval = Duration::from_millis(250);
        let stabilization = Duration::from_secs(2);
        let start = Instant::now();

        loop {
            if let Some(contract) = self.node.get_contract(&address).await? {
                let class_id = contract.current_contract_class_id;
                if self.node.get_contract_class(&class_id).await?.is_some() {
                    sleep(stabilization).await;
                    if let Some(contract) = self.node.get_contract(&address).await? {
                        if self
                            .node
                            .get_contract_class(&contract.current_contract_class_id)
                            .await?
                            .is_some()
                        {
                            return Ok(());
                        }
                    }
                }
            }

            if start.elapsed() >= timeout {
                return Err(Error::Timeout(format!(
                    "contract {address} did not become node-visible within {:?}",
                    timeout
                )));
            }

            sleep(interval).await;
        }
    }

    async fn wait_for_tx_proven(&self, tx_hash: TxHash) -> Result<(), Error> {
        wait_for_tx(
            &self.node,
            &tx_hash,
            WaitOpts {
                wait_for_status: TxStatus::Proven,
                timeout: Duration::from_secs(60),
                interval: Duration::from_millis(500),
                ..WaitOpts::default()
            },
        )
        .await
        .map(|_| ())
    }

    async fn create_auth_wit(
        &self,
        from: AztecAddress,
        message_hash_or_intent: MessageHashOrIntent,
    ) -> Result<AuthWitness, Error> {
        let chain_info = self.get_chain_info().await?;
        self.accounts
            .create_auth_wit(&from, message_hash_or_intent, &chain_info)
            .await
    }
}

/// Decode packed private events from the PXE into wallet-level [`PrivateEvent`] objects.
fn decode_private_events(
    packed: Vec<pxe::PackedPrivateEvent>,
    event_metadata: &EventMetadataDefinition,
) -> Vec<PrivateEvent> {
    let field_names = resolve_event_field_names(event_metadata);
    packed
        .into_iter()
        .map(|pe| {
            let mut event_map = serde_json::Map::new();
            for (i, name) in field_names.iter().enumerate() {
                if let Some(field) = pe.packed_event.get(i) {
                    event_map.insert(
                        name.clone(),
                        serde_json::to_value(field).unwrap_or_default(),
                    );
                }
            }

            PrivateEvent {
                event: serde_json::Value::Object(event_map),
                metadata: PrivateEventMetadata {
                    tx_hash: pe.tx_hash,
                    block_number: Some(pe.l2_block_number),
                    log_index: None,
                },
            }
        })
        .collect()
}

fn resolve_event_field_names(event_metadata: &EventMetadataDefinition) -> Vec<String> {
    if !event_metadata.field_names.is_empty() {
        return event_metadata.field_names.clone();
    }

    match &event_metadata.abi_type {
        AbiType::Struct { fields, .. } => fields.iter().map(|field| field.name.clone()).collect(),
        _ => vec![],
    }
}

/// Create a [`BaseWallet`] connected to a PXE and node.
pub fn create_wallet<P: Pxe, N: AztecNode, A: AccountProvider>(
    pxe: P,
    node: N,
    accounts: A,
) -> BaseWallet<P, N, A> {
    BaseWallet::new(pxe, node, accounts)
}

/// Create a [`BaseWallet`] backed by an embedded PXE (in-process) and HTTP node.
///
/// This is the recommended way to create a wallet for Aztec v4.x, where PXE
/// runs client-side. Only requires a single `node_url` — no separate PXE server.
#[cfg(feature = "embedded-pxe")]
pub async fn create_embedded_wallet<A: AccountProvider>(
    node_url: impl Into<String>,
    accounts: A,
) -> Result<
    BaseWallet<aztec_pxe::EmbeddedPxe<HttpNodeClient>, HttpNodeClient, A>,
    crate::error::Error,
> {
    let node = create_aztec_node_client(node_url);
    let pxe = aztec_pxe::EmbeddedPxe::create_ephemeral(node.clone()).await?;
    Ok(BaseWallet::new(pxe, node, accounts))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::abi::{AbiParameter, AbiType, EventSelector};
    use crate::fee::GasSettings;
    use crate::node::{NodeInfo, PublicLogFilter, PublicLogsResponse};
    use crate::pxe::{
        BlockHeader, ExecuteUtilityOpts, PackedPrivateEvent, ProfileTxOpts, SimulateTxOpts,
        TxExecutionRequest, TxProfileResult as PxeTxProfileResult, TxProvingResult,
        TxSimulationResult as PxeTxSimulationResult, UtilityExecutionResult as PxeUtilityResult,
    };
    use crate::tx::{TxReceipt, TxStatus};
    use crate::types::{CompleteAddress, ContractInstance, PublicKeys};
    use std::sync::Mutex;

    // -----------------------------------------------------------------------
    // Mock AztecNode
    // -----------------------------------------------------------------------

    struct MockNode {
        info: NodeInfo,
        contract: Mutex<Option<ContractInstanceWithAddress>>,
        contract_class: Mutex<Option<serde_json::Value>>,
        sent_txs: Mutex<Vec<serde_json::Value>>,
    }

    impl MockNode {
        fn new() -> Self {
            Self {
                info: NodeInfo {
                    node_version: "test-0.1.0".into(),
                    l1_chain_id: 31337,
                    rollup_version: 1,
                    enr: None,
                    l1_contract_addresses: serde_json::json!({}),
                    protocol_contract_addresses: serde_json::json!({}),
                    real_proofs: false,
                    l2_circuits_vk_tree_root: None,
                    l2_protocol_contracts_hash: None,
                },
                contract: Mutex::new(None),
                contract_class: Mutex::new(None),
                sent_txs: Mutex::new(vec![]),
            }
        }

        fn with_contract(self, c: ContractInstanceWithAddress) -> Self {
            *self.contract.lock().unwrap() = Some(c);
            self
        }

        fn with_contract_class(self, c: serde_json::Value) -> Self {
            *self.contract_class.lock().unwrap() = Some(c);
            self
        }
    }

    #[async_trait]
    impl AztecNode for MockNode {
        async fn get_node_info(&self) -> Result<NodeInfo, Error> {
            Ok(self.info.clone())
        }

        async fn get_block_number(&self) -> Result<u64, Error> {
            Ok(0)
        }

        async fn get_proven_block_number(&self) -> Result<u64, Error> {
            Ok(0)
        }

        async fn get_tx_receipt(&self, _tx_hash: &TxHash) -> Result<TxReceipt, Error> {
            Ok(TxReceipt {
                tx_hash: TxHash::zero(),
                status: TxStatus::Pending,
                execution_result: None,
                error: None,
                transaction_fee: None,
                block_hash: None,
                block_number: None,
                epoch_number: None,
            })
        }

        async fn get_tx_effect(
            &self,
            _tx_hash: &TxHash,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_tx_by_hash(
            &self,
            _tx_hash: &TxHash,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }

        async fn get_public_logs(
            &self,
            _filter: PublicLogFilter,
        ) -> Result<PublicLogsResponse, Error> {
            Ok(PublicLogsResponse {
                logs: vec![],
                max_logs_hit: false,
            })
        }

        async fn send_tx(&self, tx: &serde_json::Value) -> Result<(), Error> {
            self.sent_txs.lock().unwrap().push(tx.clone());
            Ok(())
        }

        async fn get_contract(
            &self,
            _address: &AztecAddress,
        ) -> Result<Option<ContractInstanceWithAddress>, Error> {
            Ok(self.contract.lock().unwrap().clone())
        }

        async fn get_contract_class(&self, _id: &Fr) -> Result<Option<serde_json::Value>, Error> {
            Ok(self.contract_class.lock().unwrap().clone())
        }

        async fn get_block_header(&self, _block_number: u64) -> Result<serde_json::Value, Error> {
            Ok(serde_json::json!({"blockNumber": 1}))
        }
        async fn get_block(&self, _block_number: u64) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_note_hash_membership_witness(
            &self,
            _block_number: u64,
            _note_hash: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_nullifier_membership_witness(
            &self,
            _block_number: u64,
            _nullifier: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_low_nullifier_membership_witness(
            &self,
            _block_number: u64,
            _nullifier: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_public_storage_at(
            &self,
            _block_number: u64,
            _contract: &AztecAddress,
            _slot: &Fr,
        ) -> Result<Fr, Error> {
            Ok(Fr::zero())
        }
        async fn get_public_data_witness(
            &self,
            _block_number: u64,
            _leaf_slot: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_l1_to_l2_message_membership_witness(
            &self,
            _block_number: u64,
            _entry_key: &Fr,
        ) -> Result<serde_json::Value, Error> {
            Ok(serde_json::Value::Null)
        }
        async fn simulate_public_calls(
            &self,
            _tx: &serde_json::Value,
            _skip_fee_enforcement: bool,
        ) -> Result<serde_json::Value, Error> {
            Ok(serde_json::Value::Null)
        }
        async fn is_valid_tx(
            &self,
            _tx: &serde_json::Value,
        ) -> Result<crate::node::TxValidationResult, Error> {
            Ok(crate::node::TxValidationResult::Valid)
        }
        async fn get_private_logs_by_tags(&self, _tags: &[Fr]) -> Result<serde_json::Value, Error> {
            Ok(serde_json::json!([]))
        }
        async fn get_public_logs_by_tags_from_contract(
            &self,
            _contract: &AztecAddress,
            _tags: &[Fr],
        ) -> Result<serde_json::Value, Error> {
            Ok(serde_json::json!([]))
        }
        async fn register_contract_function_signatures(
            &self,
            _signatures: &[String],
        ) -> Result<(), Error> {
            Ok(())
        }
        async fn get_block_hash_membership_witness(
            &self,
            _block_number: u64,
            _block_hash: &Fr,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn find_leaves_indexes(
            &self,
            _block_number: u64,
            _tree_id: &str,
            _leaves: &[Fr],
        ) -> Result<Vec<Option<u64>>, Error> {
            Ok(vec![])
        }
    }

    // -----------------------------------------------------------------------
    // Mock PXE
    // -----------------------------------------------------------------------

    struct MockPxe {
        senders: Mutex<Vec<AztecAddress>>,
        contract_instance: Mutex<Option<ContractInstanceWithAddress>>,
        contract_artifact: Mutex<Option<ContractArtifact>>,
        registered_contracts: Mutex<Vec<RegisterContractRequest>>,
        updated_contracts: Mutex<Vec<(AztecAddress, ContractArtifact)>>,
        registered_accounts: Mutex<Vec<(Fr, Fr)>>,
        simulate_opts: Mutex<Vec<SimulateTxOpts>>,
        prove_scopes: Mutex<Vec<Vec<AztecAddress>>>,
        profile_opts: Mutex<Vec<ProfileTxOpts>>,
        utility_opts: Mutex<Vec<ExecuteUtilityOpts>>,
        simulate_result: PxeTxSimulationResult,
        profile_result: PxeTxProfileResult,
        proving_result: TxProvingResult,
        utility_result: PxeUtilityResult,
        packed_events: Vec<PackedPrivateEvent>,
    }

    impl MockPxe {
        fn new() -> Self {
            Self {
                senders: Mutex::new(vec![]),
                contract_instance: Mutex::new(None),
                contract_artifact: Mutex::new(None),
                registered_contracts: Mutex::new(vec![]),
                updated_contracts: Mutex::new(vec![]),
                registered_accounts: Mutex::new(vec![]),
                simulate_opts: Mutex::new(vec![]),
                prove_scopes: Mutex::new(vec![]),
                profile_opts: Mutex::new(vec![]),
                utility_opts: Mutex::new(vec![]),
                simulate_result: PxeTxSimulationResult {
                    data: serde_json::json!({"returnValues": [42]}),
                },
                profile_result: PxeTxProfileResult {
                    data: serde_json::json!({"profileData": "test"}),
                },
                proving_result: TxProvingResult {
                    tx_hash: Some(TxHash::zero()),
                    private_execution_result: serde_json::json!({}),
                    public_inputs: aztec_core::tx::PrivateKernelTailCircuitPublicInputs::from_bytes(
                        vec![0],
                    ),
                    chonk_proof: aztec_core::tx::ChonkProof::from_bytes(vec![0]),
                    contract_class_log_fields: vec![],
                    public_function_calldata: vec![],
                    stats: None,
                },
                utility_result: PxeUtilityResult {
                    result: vec![Fr::from(99u64)],
                    stats: None,
                },
                packed_events: vec![],
            }
        }

        fn with_senders(self, senders: Vec<AztecAddress>) -> Self {
            *self.senders.lock().unwrap() = senders;
            self
        }

        fn with_contract_instance(self, inst: ContractInstanceWithAddress) -> Self {
            *self.contract_instance.lock().unwrap() = Some(inst);
            self
        }

        fn with_contract_artifact(self, art: ContractArtifact) -> Self {
            *self.contract_artifact.lock().unwrap() = Some(art);
            self
        }

        fn with_packed_events(mut self, events: Vec<PackedPrivateEvent>) -> Self {
            self.packed_events = events;
            self
        }
    }

    #[async_trait]
    impl Pxe for MockPxe {
        async fn get_synced_block_header(&self) -> Result<BlockHeader, Error> {
            Ok(BlockHeader {
                data: serde_json::json!({}),
            })
        }

        async fn get_contract_instance(
            &self,
            _address: &AztecAddress,
        ) -> Result<Option<ContractInstanceWithAddress>, Error> {
            Ok(self.contract_instance.lock().unwrap().clone())
        }

        async fn get_contract_artifact(&self, _id: &Fr) -> Result<Option<ContractArtifact>, Error> {
            Ok(self.contract_artifact.lock().unwrap().clone())
        }

        async fn get_contracts(&self) -> Result<Vec<AztecAddress>, Error> {
            Ok(vec![])
        }

        async fn register_account(
            &self,
            secret_key: &Fr,
            partial_address: &Fr,
        ) -> Result<CompleteAddress, Error> {
            self.registered_accounts
                .lock()
                .unwrap()
                .push((*secret_key, *partial_address));
            Ok(CompleteAddress {
                address: AztecAddress(Fr::from(1u64)),
                public_keys: PublicKeys::default(),
                partial_address: *partial_address,
            })
        }

        async fn get_registered_accounts(&self) -> Result<Vec<CompleteAddress>, Error> {
            Ok(vec![])
        }

        async fn register_sender(&self, sender: &AztecAddress) -> Result<AztecAddress, Error> {
            self.senders.lock().unwrap().push(*sender);
            Ok(*sender)
        }

        async fn get_senders(&self) -> Result<Vec<AztecAddress>, Error> {
            Ok(self.senders.lock().unwrap().clone())
        }

        async fn remove_sender(&self, _sender: &AztecAddress) -> Result<(), Error> {
            Ok(())
        }

        async fn register_contract_class(&self, _artifact: &ContractArtifact) -> Result<(), Error> {
            Ok(())
        }

        async fn register_contract(&self, request: RegisterContractRequest) -> Result<(), Error> {
            self.registered_contracts.lock().unwrap().push(request);
            Ok(())
        }

        async fn update_contract(
            &self,
            address: &AztecAddress,
            artifact: &ContractArtifact,
        ) -> Result<(), Error> {
            self.updated_contracts
                .lock()
                .unwrap()
                .push((*address, artifact.clone()));
            Ok(())
        }

        async fn simulate_tx(
            &self,
            _tx_request: &TxExecutionRequest,
            opts: SimulateTxOpts,
        ) -> Result<PxeTxSimulationResult, Error> {
            self.simulate_opts.lock().unwrap().push(opts);
            Ok(self.simulate_result.clone())
        }

        async fn prove_tx(
            &self,
            _tx_request: &TxExecutionRequest,
            scopes: Vec<AztecAddress>,
        ) -> Result<TxProvingResult, Error> {
            self.prove_scopes.lock().unwrap().push(scopes);
            Ok(self.proving_result.clone())
        }

        async fn profile_tx(
            &self,
            _tx_request: &TxExecutionRequest,
            opts: ProfileTxOpts,
        ) -> Result<PxeTxProfileResult, Error> {
            self.profile_opts.lock().unwrap().push(opts);
            Ok(self.profile_result.clone())
        }

        async fn execute_utility(
            &self,
            _call: &FunctionCall,
            opts: ExecuteUtilityOpts,
        ) -> Result<PxeUtilityResult, Error> {
            self.utility_opts.lock().unwrap().push(opts);
            Ok(self.utility_result.clone())
        }

        async fn get_private_events(
            &self,
            _event_selector: &EventSelector,
            _filter: pxe::PrivateEventFilter,
        ) -> Result<Vec<PackedPrivateEvent>, Error> {
            Ok(self.packed_events.clone())
        }

        async fn stop(&self) -> Result<(), Error> {
            Ok(())
        }
    }

    // -----------------------------------------------------------------------
    // Mock AccountProvider
    // -----------------------------------------------------------------------

    struct MockAccountProvider {
        accounts: Vec<Aliased<AztecAddress>>,
        complete_addresses: Vec<CompleteAddress>,
        created_execs: Mutex<Vec<ExecutionPayload>>,
    }

    impl MockAccountProvider {
        fn new(accounts: Vec<Aliased<AztecAddress>>) -> Self {
            Self {
                accounts,
                complete_addresses: vec![],
                created_execs: Mutex::new(vec![]),
            }
        }

        fn single(address: AztecAddress) -> Self {
            Self {
                accounts: vec![Aliased {
                    alias: "test".to_owned(),
                    item: address,
                }],
                complete_addresses: vec![],
                created_execs: Mutex::new(vec![]),
            }
        }

        fn single_with_complete(address: AztecAddress, partial_address: Fr) -> Self {
            Self {
                accounts: vec![Aliased {
                    alias: "test".to_owned(),
                    item: address,
                }],
                complete_addresses: vec![CompleteAddress {
                    address,
                    public_keys: PublicKeys::default(),
                    partial_address,
                }],
                created_execs: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait]
    impl AccountProvider for MockAccountProvider {
        async fn create_tx_execution_request(
            &self,
            from: &AztecAddress,
            exec: ExecutionPayload,
            _gas_settings: GasSettings,
            _chain_info: &ChainInfo,
            _fee_payer: Option<AztecAddress>,
        ) -> Result<TxExecutionRequest, Error> {
            if !self.accounts.iter().any(|a| a.item == *from) {
                return Err(Error::InvalidData(format!("account not found: {from}")));
            }
            self.created_execs.lock().unwrap().push(exec);
            Ok(TxExecutionRequest {
                data: serde_json::json!({
                    "origin": from.to_string(),
                    "calls": [],
                }),
            })
        }

        async fn create_auth_wit(
            &self,
            from: &AztecAddress,
            _intent: MessageHashOrIntent,
            _chain_info: &ChainInfo,
        ) -> Result<AuthWitness, Error> {
            if !self.accounts.iter().any(|a| a.item == *from) {
                return Err(Error::InvalidData(format!("account not found: {from}")));
            }
            Ok(AuthWitness {
                fields: vec![Fr::from(1u64), Fr::from(2u64)],
                ..Default::default()
            })
        }

        async fn get_complete_address(
            &self,
            address: &AztecAddress,
        ) -> Result<Option<CompleteAddress>, Error> {
            Ok(self
                .complete_addresses
                .iter()
                .find(|complete| complete.address == *address)
                .cloned())
        }

        async fn get_accounts(&self) -> Result<Vec<Aliased<AztecAddress>>, Error> {
            Ok(self.accounts.clone())
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn sample_instance() -> ContractInstanceWithAddress {
        ContractInstanceWithAddress {
            address: AztecAddress(Fr::from(1u64)),
            inner: ContractInstance {
                version: 1,
                salt: Fr::from(42u64),
                deployer: AztecAddress(Fr::from(2u64)),
                current_contract_class_id: Fr::from(100u64),
                original_contract_class_id: Fr::from(100u64),
                initialization_hash: Fr::from(0u64),
                public_keys: PublicKeys::default(),
            },
        }
    }

    fn sample_artifact() -> ContractArtifact {
        ContractArtifact {
            name: "TestContract".to_owned(),
            functions: vec![],
            outputs: None,
            file_map: None,
            context_inputs_sizes: None,
        }
    }

    fn test_address() -> AztecAddress {
        AztecAddress(Fr::from(1u64))
    }

    fn make_wallet(
        pxe: MockPxe,
        node: MockNode,
        accounts: MockAccountProvider,
    ) -> BaseWallet<MockPxe, MockNode, MockAccountProvider> {
        BaseWallet::new(pxe, node, accounts)
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[test]
    fn base_wallet_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<BaseWallet<MockPxe, MockNode, MockAccountProvider>>();
    }

    #[tokio::test]
    async fn test_get_chain_info() {
        let wallet = make_wallet(
            MockPxe::new(),
            MockNode::new(),
            MockAccountProvider::new(vec![]),
        );
        let info = wallet.get_chain_info().await.expect("get chain info");
        assert_eq!(info.chain_id, Fr::from(31337u64));
        assert_eq!(info.version, Fr::from(1u64));
    }

    #[tokio::test]
    async fn test_get_accounts() {
        let addr = test_address();
        let wallet = make_wallet(
            MockPxe::new(),
            MockNode::new(),
            MockAccountProvider::single(addr),
        );
        let accounts = wallet.get_accounts().await.expect("get accounts");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].item, addr);
        assert_eq!(accounts[0].alias, "test");
    }

    #[tokio::test]
    async fn test_get_address_book() {
        let senders = vec![AztecAddress(Fr::from(10u64)), AztecAddress(Fr::from(20u64))];
        let wallet = make_wallet(
            MockPxe::new().with_senders(senders.clone()),
            MockNode::new(),
            MockAccountProvider::new(vec![]),
        );
        let book = wallet.get_address_book().await.expect("get address book");
        assert_eq!(book.len(), 2);
        assert_eq!(book[0].item, senders[0]);
        assert!(book[0].alias.is_empty());
    }

    #[tokio::test]
    async fn test_register_sender() {
        let wallet = make_wallet(
            MockPxe::new(),
            MockNode::new(),
            MockAccountProvider::new(vec![]),
        );
        let addr = AztecAddress(Fr::from(99u64));
        let result = wallet
            .register_sender(addr, Some("bob".into()))
            .await
            .expect("register sender");
        assert_eq!(result, addr);
    }

    #[tokio::test]
    async fn test_register_contract() {
        let instance = sample_instance();
        let artifact = sample_artifact();
        let wallet = make_wallet(
            MockPxe::new(),
            MockNode::new(),
            MockAccountProvider::new(vec![]),
        );
        let result = wallet
            .register_contract(instance.clone(), Some(artifact.clone()), None)
            .await
            .expect("register contract");
        assert_eq!(result.address, instance.address);

        let registered = wallet.pxe.registered_contracts.lock().unwrap();
        assert_eq!(registered.len(), 1);
        assert_eq!(registered[0].instance.address, instance.address);
        assert_eq!(
            registered[0]
                .artifact
                .as_ref()
                .expect("artifact attached")
                .name,
            artifact.name,
        );
    }

    #[tokio::test]
    async fn test_register_contract_with_secret_key() {
        let partial_address = Fr::from(777u64);
        let wallet = make_wallet(
            MockPxe::new(),
            MockNode::new(),
            MockAccountProvider::single_with_complete(test_address(), partial_address),
        );
        let instance = sample_instance();
        let sk = Fr::from(12345u64);
        wallet
            .register_contract(instance.clone(), Some(sample_artifact()), Some(sk))
            .await
            .expect("register contract with sk");

        // Account registration must use the managed partial address from the
        // account provider, not a zero or caller-supplied value.
        let accounts = wallet.pxe.registered_accounts.lock().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].0, sk);
        assert_eq!(accounts[0].1, partial_address);
    }

    #[tokio::test]
    async fn test_register_contract_falls_back_to_pxe_artifact() {
        let wallet = make_wallet(
            MockPxe::new().with_contract_artifact(sample_artifact()),
            MockNode::new(),
            MockAccountProvider::new(vec![]),
        );
        let instance = sample_instance();
        wallet
            .register_contract(instance.clone(), None, None)
            .await
            .expect("register with PXE-stored artifact");

        let registered = wallet.pxe.registered_contracts.lock().unwrap();
        assert_eq!(registered.len(), 1);
        assert_eq!(
            registered[0]
                .artifact
                .as_ref()
                .expect("artifact resolved from PXE")
                .name,
            "TestContract",
        );
    }

    #[tokio::test]
    async fn test_register_contract_reuses_existing_registration() {
        let wallet = make_wallet(
            MockPxe::new().with_contract_instance(sample_instance()),
            MockNode::new(),
            MockAccountProvider::new(vec![]),
        );

        wallet
            .register_contract(sample_instance(), None, None)
            .await
            .expect("reuse existing registration");

        assert!(wallet.pxe.registered_contracts.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_register_contract_updates_existing_registration() {
        let existing = sample_instance();
        let mut updated = sample_instance();
        updated.inner.current_contract_class_id = Fr::from(200u64);
        let artifact = sample_artifact();
        let wallet = make_wallet(
            MockPxe::new().with_contract_instance(existing),
            MockNode::new(),
            MockAccountProvider::new(vec![]),
        );

        wallet
            .register_contract(updated.clone(), Some(artifact.clone()), None)
            .await
            .expect("update existing registration");

        let updated_contracts = wallet.pxe.updated_contracts.lock().unwrap();
        assert_eq!(updated_contracts.len(), 1);
        assert_eq!(updated_contracts[0].0, updated.address);
        assert_eq!(updated_contracts[0].1.name, artifact.name);
    }

    #[tokio::test]
    async fn test_get_contract_metadata_not_published() {
        let instance = sample_instance();
        let wallet = make_wallet(
            MockPxe::new().with_contract_instance(instance.clone()),
            MockNode::new(),
            MockAccountProvider::new(vec![]),
        );
        let meta = wallet
            .get_contract_metadata(instance.address)
            .await
            .expect("get contract metadata");
        assert!(meta.instance.is_some());
        assert!(!meta.is_contract_published);
        assert!(!meta.is_contract_initialized);
    }

    #[tokio::test]
    async fn test_get_contract_metadata_published() {
        let instance = sample_instance();
        let wallet = make_wallet(
            MockPxe::new().with_contract_instance(instance.clone()),
            MockNode::new().with_contract(instance.clone()),
            MockAccountProvider::new(vec![]),
        );
        let meta = wallet
            .get_contract_metadata(instance.address)
            .await
            .expect("get contract metadata");
        assert!(meta.instance.is_some());
        assert!(meta.is_contract_published);
        assert!(meta.is_contract_initialized);
    }

    #[tokio::test]
    async fn test_get_contract_metadata_updated() {
        let instance = sample_instance();
        let mut on_chain = sample_instance();
        on_chain.inner.current_contract_class_id = Fr::from(200u64);
        let wallet = make_wallet(
            MockPxe::new().with_contract_instance(instance),
            MockNode::new().with_contract(on_chain),
            MockAccountProvider::new(vec![]),
        );

        let meta = wallet
            .get_contract_metadata(test_address())
            .await
            .expect("get updated contract metadata");
        assert!(meta.is_contract_updated);
        assert_eq!(meta.updated_contract_class_id, Some(Fr::from(200u64)));
    }

    #[tokio::test]
    async fn test_get_contract_class_metadata() {
        let art = sample_artifact();
        let wallet = make_wallet(
            MockPxe::new().with_contract_artifact(art),
            MockNode::new().with_contract_class(serde_json::json!({"id": "0x64"})),
            MockAccountProvider::new(vec![]),
        );
        let meta = wallet
            .get_contract_class_metadata(Fr::from(100u64))
            .await
            .expect("get contract class metadata");
        assert!(meta.is_artifact_registered);
        assert!(meta.is_contract_class_publicly_registered);
    }

    #[tokio::test]
    async fn test_get_contract_class_metadata_not_registered() {
        let wallet = make_wallet(
            MockPxe::new(),
            MockNode::new(),
            MockAccountProvider::new(vec![]),
        );
        let meta = wallet
            .get_contract_class_metadata(Fr::from(100u64))
            .await
            .expect("get contract class metadata");
        assert!(!meta.is_artifact_registered);
        assert!(!meta.is_contract_class_publicly_registered);
    }

    #[tokio::test]
    async fn test_simulate_tx() {
        let addr = test_address();
        let wallet = make_wallet(
            MockPxe::new(),
            MockNode::new(),
            MockAccountProvider::single(addr),
        );
        let result = wallet
            .simulate_tx(
                ExecutionPayload::default(),
                SimulateOptions {
                    from: addr,
                    ..Default::default()
                },
            )
            .await
            .expect("simulate tx");
        assert_eq!(
            result.return_values,
            serde_json::json!({"returnValues": [42]})
        );

        let simulate_opts = wallet.pxe.simulate_opts.lock().unwrap();
        assert_eq!(simulate_opts.len(), 1);
        assert!(simulate_opts[0].skip_fee_enforcement);
    }

    #[tokio::test]
    async fn test_send_tx() {
        let addr = test_address();
        let wallet = make_wallet(
            MockPxe::new(),
            MockNode::new(),
            MockAccountProvider::single(addr),
        );
        let result = wallet
            .send_tx(
                ExecutionPayload::default(),
                SendOptions {
                    from: addr,
                    ..Default::default()
                },
            )
            .await
            .expect("send tx");
        assert_eq!(
            result.tx_hash,
            TxHash::from_hex("0x00000000000000000000000000000000000000000000000000000000deadbeef")
                .unwrap()
        );

        // Verify node received the proven tx
        let sent = wallet.node.sent_txs.lock().unwrap();
        assert_eq!(sent.len(), 1);

        let scopes = wallet.pxe.prove_scopes.lock().unwrap();
        assert_eq!(scopes.as_slice(), &[vec![addr]]);
    }

    #[tokio::test]
    async fn test_create_auth_wit() {
        let addr = test_address();
        let wallet = make_wallet(
            MockPxe::new(),
            MockNode::new(),
            MockAccountProvider::single(addr),
        );
        let wit = wallet
            .create_auth_wit(
                addr,
                MessageHashOrIntent::Hash {
                    hash: Fr::from(42u64),
                },
            )
            .await
            .expect("create auth wit");
        assert_eq!(wit.fields.len(), 2);
        assert_eq!(wit.fields[0], Fr::from(1u64));
    }

    #[tokio::test]
    async fn test_create_auth_wit_unknown_account() {
        let wallet = make_wallet(
            MockPxe::new(),
            MockNode::new(),
            MockAccountProvider::new(vec![]),
        );
        let result = wallet
            .create_auth_wit(
                AztecAddress(Fr::from(999u64)),
                MessageHashOrIntent::Hash {
                    hash: Fr::from(1u64),
                },
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_utility() {
        let wallet = make_wallet(
            MockPxe::new(),
            MockNode::new(),
            MockAccountProvider::new(vec![]),
        );
        let call = FunctionCall {
            to: AztecAddress(Fr::from(1u64)),
            selector: crate::abi::FunctionSelector::from_hex("0xaabbccdd").expect("valid selector"),
            args: vec![],
            function_type: crate::abi::FunctionType::Utility,
            is_static: true,
            hide_msg_sender: false,
        };
        let result = wallet
            .execute_utility(call, ExecuteUtilityOptions::default())
            .await
            .expect("execute utility");
        assert_ne!(result.result, serde_json::Value::Null);

        let utility_opts = wallet.pxe.utility_opts.lock().unwrap();
        assert_eq!(
            utility_opts.as_slice(),
            &[ExecuteUtilityOpts {
                authwits: vec![],
                scopes: vec![AztecAddress(Fr::zero())],
            }]
        );
    }

    #[tokio::test]
    async fn test_profile_tx() {
        let addr = test_address();
        let wallet = make_wallet(
            MockPxe::new(),
            MockNode::new(),
            MockAccountProvider::single(addr),
        );
        let result = wallet
            .profile_tx(
                ExecutionPayload::default(),
                ProfileOptions {
                    from: addr,
                    ..Default::default()
                },
            )
            .await
            .expect("profile tx");
        assert_ne!(result.profile_data, serde_json::Value::Null);

        let profile_opts = wallet.pxe.profile_opts.lock().unwrap();
        assert_eq!(profile_opts.len(), 1);
        assert!(profile_opts[0].skip_proof_generation);
    }

    #[tokio::test]
    async fn test_wallet_options_are_merged_into_execution_payload() {
        let addr = test_address();
        let wallet = make_wallet(
            MockPxe::new(),
            MockNode::new(),
            MockAccountProvider::single(addr),
        );

        wallet
            .simulate_tx(
                ExecutionPayload::default(),
                SimulateOptions {
                    from: addr,
                    auth_witnesses: vec![AuthWitness {
                        fields: vec![Fr::from(9u64)],
                        ..Default::default()
                    }],
                    capsules: vec![crate::tx::Capsule {
                        contract_address: AztecAddress(Fr::zero()),
                        storage_slot: Fr::zero(),
                        data: vec![Fr::from(1u64), Fr::from(2u64), Fr::from(3u64)],
                    }],
                    ..Default::default()
                },
            )
            .await
            .expect("simulate tx with wallet options");

        let created_execs = wallet.accounts.created_execs.lock().unwrap();
        assert_eq!(created_execs.len(), 1);
        assert_eq!(created_execs[0].auth_witnesses.len(), 1);
        assert_eq!(created_execs[0].capsules.len(), 1);
    }

    #[tokio::test]
    async fn test_get_private_events() {
        let packed = vec![PackedPrivateEvent {
            packed_event: vec![Fr::from(100u64), Fr::from(200u64)],
            tx_hash: TxHash::zero(),
            l2_block_number: 5,
            l2_block_hash: pxe::BlockHash::default(),
            event_selector: EventSelector(Fr::from(1u64)),
        }];
        let wallet = make_wallet(
            MockPxe::new().with_packed_events(packed),
            MockNode::new(),
            MockAccountProvider::new(vec![]),
        );

        let event_metadata = EventMetadataDefinition {
            event_selector: EventSelector(Fr::from(1u64)),
            abi_type: AbiType::Struct {
                name: "Transfer".to_owned(),
                fields: vec![
                    AbiParameter {
                        name: "amount".to_owned(),
                        typ: AbiType::Field,
                        visibility: None,
                    },
                    AbiParameter {
                        name: "sender".to_owned(),
                        typ: AbiType::Field,
                        visibility: None,
                    },
                ],
            },
            field_names: vec!["amount".to_owned(), "sender".to_owned()],
        };

        let events = wallet
            .get_private_events(
                &event_metadata,
                PrivateEventFilter {
                    contract_address: AztecAddress(Fr::from(1u64)),
                    ..Default::default()
                },
            )
            .await
            .expect("get private events");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].metadata.block_number, Some(5));
        assert_eq!(events[0].metadata.tx_hash, TxHash::zero());

        // Verify decoded fields
        let event = &events[0].event;
        assert!(event.get("amount").is_some());
        assert!(event.get("sender").is_some());
    }

    #[tokio::test]
    async fn test_create_wallet_factory() {
        let wallet = create_wallet(
            MockPxe::new(),
            MockNode::new(),
            MockAccountProvider::new(vec![]),
        );
        let info = wallet.get_chain_info().await.expect("get chain info");
        assert_eq!(info.chain_id, Fr::from(31337u64));
    }
}
