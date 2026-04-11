//! Embedded PXE implementation that runs PXE logic in-process.

use std::sync::Arc;

use async_trait::async_trait;
use aztec_core::abi::FunctionSelector;
use aztec_core::abi::{abi_type_signature, ContractArtifact, EventSelector, FunctionType};
use aztec_core::constants::{
    contract_class_published_magic_value, contract_instance_published_magic_value,
    current_vk_tree_root, protocol_contract_address, MAX_PACKED_PUBLIC_BYTECODE_SIZE_IN_FIELDS,
};
use aztec_core::error::Error;
use aztec_core::hash::{
    compute_contract_address_from_instance, compute_contract_class_id,
    compute_contract_class_id_from_artifact, compute_protocol_contracts_hash,
    compute_protocol_nullifier,
};
use aztec_core::tx::{compute_tx_request_hash, Capsule, FunctionCall, TxContext};
use aztec_core::types::{
    AztecAddress, CompleteAddress, ContractInstance, ContractInstanceWithAddress, Fr, Point,
    PublicKeys,
};
use aztec_crypto::complete_address_from_secret_key_and_partial_address;
use aztec_crypto::schnorr::{schnorr_verify, SchnorrSignature};
use aztec_node_client::AztecNode;
use aztec_pxe_client::{
    BlockHeader, ExecuteUtilityOpts, PackedPrivateEvent, PrivateEventFilter, ProfileTxOpts, Pxe,
    RegisterContractRequest, SimulateTxOpts, TxExecutionRequest, TxProfileResult, TxProvingResult,
    TxSimulationResult, UtilityExecutionResult,
};

use crate::kernel::prover::{BbPrivateKernelProver, BbProverConfig};
use crate::stores::anchor_block_store::AnchorBlockHeader;
use crate::stores::kv::KvStore;
use crate::stores::{
    AddressStore, AnchorBlockStore, CapsuleStore, ContractStore, KeyStore, NoteStore,
    PrivateEventStore, RecipientTaggingStore, SenderStore, SenderTaggingStore,
};
use crate::sync::block_state_synchronizer::{BlockStateSynchronizer, BlockSyncConfig};
use crate::sync::event_filter::PrivateEventFilterValidator;
use crate::sync::ContractSyncService;

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DecodedTxExecutionRequest {
    origin: AztecAddress,
    first_call_args_hash: Fr,
    args_of_calls: Vec<aztec_core::tx::HashedValues>,
    #[serde(default)]
    fee_payer: Option<AztecAddress>,
}

#[derive(Debug, Clone, Copy)]
struct ParsedEntrypointCall {
    args_hash: Fr,
    selector: aztec_core::abi::FunctionSelector,
    to: AztecAddress,
    is_public: bool,
    hide_msg_sender: bool,
    is_static: bool,
}

#[derive(Debug)]
struct DecodedEntrypointCall {
    to: AztecAddress,
    selector: aztec_core::abi::FunctionSelector,
    encoded_args: Vec<Fr>,
    hide_msg_sender: bool,
    is_static: bool,
}

#[derive(Debug, Clone)]
struct CallExecutionBundle {
    execution_result: crate::execution::execution_result::PrivateExecutionResult,
    contract_class_log_fields: Vec<aztec_core::tx::ContractClassLogFields>,
    public_function_calldata: Vec<aztec_core::tx::HashedValues>,
    /// Return values from the first inner ACIR call (for private return value extraction).
    first_acir_call_return_values: Vec<Fr>,
    /// User-visible return values for the top-level simulated call bundle.
    simulated_return_values: Vec<Fr>,
}

fn parse_encoded_calls(fields: &[Fr]) -> Result<Vec<ParsedEntrypointCall>, Error> {
    const CALL_FIELDS: usize = 6;
    const APP_MAX_CALLS: usize = 5;
    let required = CALL_FIELDS * APP_MAX_CALLS + 1;
    if fields.len() < required {
        return Err(Error::InvalidData(format!(
            "entrypoint args too short: {} < {}",
            fields.len(),
            required
        )));
    }

    let mut calls = Vec::with_capacity(APP_MAX_CALLS);
    for idx in 0..APP_MAX_CALLS {
        let offset = idx * CALL_FIELDS;
        calls.push(ParsedEntrypointCall {
            args_hash: fields[offset],
            selector: aztec_core::abi::FunctionSelector::from_field(fields[offset + 1]),
            to: AztecAddress(fields[offset + 2]),
            is_public: fields[offset + 3] != Fr::zero(),
            hide_msg_sender: fields[offset + 4] != Fr::zero(),
            is_static: fields[offset + 5] != Fr::zero(),
        });
    }
    Ok(calls)
}

/// Embedded PXE that runs private execution logic in-process.
///
/// In-process PXE for Aztec v4.x where PXE runs client-side.
/// Talks to the Aztec node via `node_*` RPC methods and maintains local
/// stores for contracts, keys, addresses, notes, capsules, and events.
///
/// Phase 3 additions: anchor block tracking, block reorg handling,
/// private event retrieval, transaction profiling, and persistent storage.
pub struct EmbeddedPxe<N: AztecNode> {
    node: N,
    contract_store: ContractStore,
    key_store: KeyStore,
    address_store: AddressStore,
    note_store: Arc<NoteStore>,
    #[allow(dead_code)] // Used when ACVM integration is complete
    capsule_store: CapsuleStore,
    /// Registered sender addresses for private log discovery.
    sender_store: SenderStore,
    /// Sender tagging store for outgoing tag index tracking.
    #[allow(dead_code)] // Used when full prove_tx flow is wired
    sender_tagging_store: SenderTaggingStore,
    /// Recipient tagging store for incoming tag index tracking.
    #[allow(dead_code)] // Used when full prove_tx flow is wired
    recipient_tagging_store: RecipientTaggingStore,
    /// Private event store for discovered private events.
    private_event_store: Arc<PrivateEventStore>,
    /// Kernel prover for generating proofs via bb binary.
    #[allow(dead_code)] // Used when full prove_tx flow is wired
    kernel_prover: BbPrivateKernelProver,
    /// Anchor block store for persistent block header tracking.
    anchor_block_store: Arc<AnchorBlockStore>,
    /// Block state synchronizer with reorg handling.
    block_synchronizer: BlockStateSynchronizer,
    /// Contract sync service for note discovery caching.
    contract_sync_service: ContractSyncService<N>,
    /// VK tree root from node info — needed in TxConstantData.
    vk_tree_root: Fr,
    /// Protocol contracts hash from node info — needed in TxConstantData.
    protocol_contracts_hash: Fr,
}

/// Configuration for EmbeddedPxe creation.
#[derive(Debug, Clone)]
pub struct EmbeddedPxeConfig {
    /// BB prover configuration.
    pub prover_config: BbProverConfig,
    /// Block synchronization configuration.
    pub block_sync_config: BlockSyncConfig,
}

impl Default for EmbeddedPxeConfig {
    fn default() -> Self {
        Self {
            prover_config: BbProverConfig::default(),
            block_sync_config: BlockSyncConfig::default(),
        }
    }
}

impl<N: AztecNode + Clone + 'static> EmbeddedPxe<N> {
    async fn execute_sync_state_for_contract(
        &self,
        contract_address: AztecAddress,
        scopes: Vec<AztecAddress>,
    ) -> Result<(), Error> {
        let Some(instance) = self.contract_store.get_instance(&contract_address).await? else {
            return Ok(());
        };
        let Some(artifact) = self
            .contract_store
            .get_artifact(&instance.inner.current_contract_class_id)
            .await?
        else {
            return Ok(());
        };

        let Ok(function) = artifact.find_function("sync_state") else {
            return Ok(());
        };
        if function.function_type != FunctionType::Utility {
            return Ok(());
        }

        let selector = function.selector.ok_or_else(|| {
            Error::InvalidData(format!(
                "sync_state missing selector in artifact {}",
                artifact.name
            ))
        })?;

        let call = FunctionCall {
            to: contract_address,
            selector,
            args: vec![],
            function_type: FunctionType::Utility,
            is_static: function.is_static,
            hide_msg_sender: false,
        };

        self.execute_utility(
            &call,
            ExecuteUtilityOpts {
                scopes,
                ..Default::default()
            },
        )
        .await?;
        Ok(())
    }

    async fn persist_pending_notes(
        &self,
        exec_result: &crate::execution::PrivateExecutionResult,
        scopes: &[AztecAddress],
    ) -> Result<(), Error> {
        let nullifiers_by_counter: std::collections::HashMap<u32, Fr> = exec_result
            .all_nullifiers()
            .into_iter()
            .map(|n| (n.nullifier.counter, n.nullifier.value))
            .collect();
        let note_to_nullifier_counter = exec_result.all_note_hash_nullifier_counter_maps();

        for call in exec_result.iter_all_calls() {
            for note in &call.new_notes {
                // Check if this note was also nullified in the same execution
                // (transient note — squashed by the kernel).
                let (siloed_nullifier, nullified) = if let Some(nullifier_counter) =
                    note_to_nullifier_counter.get(&note.counter).copied()
                {
                    if let Some(sn) = nullifiers_by_counter.get(&nullifier_counter).copied() {
                        (sn, true)
                    } else {
                        (Fr::zero(), false)
                    }
                } else {
                    (Fr::zero(), false)
                };

                let stored = crate::stores::note_store::StoredNote {
                    contract_address: note.contract_address,
                    owner: note.owner,
                    storage_slot: note.storage_slot,
                    randomness: note.randomness,
                    note_nonce: Fr::zero(),
                    note_hash: note.note_hash,
                    siloed_nullifier,
                    note_data: note.note_items.clone(),
                    nullified,
                    is_pending: true,
                    nullification_block_number: None,
                    leaf_index: None,
                    block_number: None,
                    tx_index_in_block: None,
                    note_index_in_tx: None,
                    scopes: vec![],
                };

                let mut stored_for_owner = false;
                for scope in scopes {
                    if *scope == note.owner {
                        self.note_store.add_notes(&[stored.clone()], scope).await?;
                        stored_for_owner = true;
                    }
                }

                if !stored_for_owner {
                    let fallback_scope = if !note.owner.0.is_zero() {
                        note.owner
                    } else {
                        AztecAddress::zero()
                    };
                    self.note_store
                        .add_notes(&[stored], &fallback_scope)
                        .await?;
                }
            }
        }

        Ok(())
    }

    fn tx_request_hash(tx_request: &TxExecutionRequest) -> Result<Fr, Error> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct WireTxRequest {
            origin: AztecAddress,
            function_selector: FunctionSelector,
            first_call_args_hash: Fr,
            tx_context: TxContext,
            salt: Fr,
        }

        let request: WireTxRequest = serde_json::from_value(tx_request.data.clone())
            .map_err(|err| Error::InvalidData(format!("invalid tx request payload: {err}")))?;

        Ok(compute_tx_request_hash(
            request.origin,
            request.first_call_args_hash,
            &request.tx_context,
            request.function_selector,
            true, // isPrivate — the account entrypoint is a private function
            request.salt,
        ))
    }

    /// Create a new EmbeddedPxe backed by the given node client and KV store.
    pub async fn create(node: N, kv: Arc<dyn KvStore>) -> Result<Self, Error> {
        Self::create_with_config(node, kv, EmbeddedPxeConfig::default()).await
    }

    /// Create a new EmbeddedPxe with custom BB prover configuration.
    pub async fn create_with_prover_config(
        node: N,
        kv: Arc<dyn KvStore>,
        prover_config: BbProverConfig,
    ) -> Result<Self, Error> {
        Self::create_with_config(
            node,
            kv,
            EmbeddedPxeConfig {
                prover_config,
                ..Default::default()
            },
        )
        .await
    }

    /// Create a new EmbeddedPxe with full configuration.
    pub async fn create_with_config(
        node: N,
        kv: Arc<dyn KvStore>,
        config: EmbeddedPxeConfig,
    ) -> Result<Self, Error> {
        let contract_store = ContractStore::new(Arc::clone(&kv));
        let key_store = KeyStore::new(Arc::clone(&kv));
        let address_store = AddressStore::new(Arc::clone(&kv));
        let note_store = Arc::new(NoteStore::new(Arc::clone(&kv)));
        let capsule_store = CapsuleStore::new(Arc::clone(&kv));
        let sender_store = SenderStore::new(Arc::clone(&kv));
        let sender_tagging_store = SenderTaggingStore::new(Arc::clone(&kv));
        let recipient_tagging_store = RecipientTaggingStore::new(Arc::clone(&kv));
        let private_event_store = Arc::new(PrivateEventStore::new(Arc::clone(&kv)));
        let anchor_block_store = Arc::new(AnchorBlockStore::new(Arc::clone(&kv)));
        let kernel_prover = BbPrivateKernelProver::new(config.prover_config);

        // Block state synchronizer with reorg handling
        let block_synchronizer = BlockStateSynchronizer::new(
            Arc::clone(&anchor_block_store),
            Arc::clone(&note_store),
            Arc::clone(&private_event_store),
            config.block_sync_config,
        );

        // Contract sync service — needs a shared reference to the node
        let contract_sync_service =
            ContractSyncService::new(Arc::new(node.clone()), Arc::clone(&note_store));

        // The node API does not expose these fields on Aztec v4.x.
        // Match upstream PXE behavior by deriving them locally and only
        // honoring node-provided values when they are explicitly present.
        let node_info = node.get_node_info().await?;
        let vk_tree_root = node_info
            .l2_circuits_vk_tree_root
            .as_deref()
            .and_then(|s| Fr::from_hex(s).ok())
            .unwrap_or_else(current_vk_tree_root);
        let protocol_contracts_hash = node_info
            .l2_protocol_contracts_hash
            .as_deref()
            .and_then(|s| Fr::from_hex(s).ok())
            .unwrap_or_else(compute_protocol_contracts_hash);

        let pxe = Self {
            node,
            contract_store,
            key_store,
            address_store,
            note_store,
            capsule_store,
            sender_store,
            sender_tagging_store,
            recipient_tagging_store,
            private_event_store,
            kernel_prover,
            anchor_block_store,
            block_synchronizer,
            contract_sync_service,
            vk_tree_root,
            protocol_contracts_hash,
        };

        // Initial block sync
        pxe.block_synchronizer.sync(&pxe.node).await?;

        Ok(pxe)
    }

    /// Create a new EmbeddedPxe with an in-memory KV store.
    pub async fn create_ephemeral(node: N) -> Result<Self, Error> {
        let kv = Arc::new(crate::stores::InMemoryKvStore::new());
        Self::create(node, kv).await
    }

    /// Recursively flatten an [`AbiValue`] into field elements.
    fn abi_value_to_fields(v: &aztec_core::abi::AbiValue) -> Vec<Fr> {
        match v {
            aztec_core::abi::AbiValue::Field(f) => vec![*f],
            aztec_core::abi::AbiValue::Integer(i) => vec![Fr::from(*i as u64)],
            aztec_core::abi::AbiValue::Boolean(b) => vec![Fr::from(*b)],
            aztec_core::abi::AbiValue::String(s) => s.chars().map(|c| Fr::from(c as u64)).collect(),
            aztec_core::abi::AbiValue::Array(arr) => {
                arr.iter().flat_map(Self::abi_value_to_fields).collect()
            }
            aztec_core::abi::AbiValue::Struct(fields) => fields
                .values()
                .flat_map(Self::abi_value_to_fields)
                .collect(),
            aztec_core::abi::AbiValue::Tuple(elems) => {
                elems.iter().flat_map(Self::abi_value_to_fields).collect()
            }
        }
    }

    /// Sync the block state from the node, handling reorgs.
    ///
    /// After sync, if the anchor block changed, wipe the contract sync cache.
    async fn sync_block_state(&self) -> Result<(), Error> {
        self.block_synchronizer.sync(&self.node).await?;

        // If anchor changed, wipe contract sync cache
        if self.block_synchronizer.take_anchor_changed().await {
            self.contract_sync_service.wipe().await;
        }

        Ok(())
    }

    /// Get the current anchor block header, syncing if necessary.
    async fn get_anchor_block_header(&self) -> Result<AnchorBlockHeader, Error> {
        self.sync_block_state().await?;
        self.block_synchronizer
            .get_anchor_block_header()
            .await?
            .ok_or_else(|| Error::InvalidData("anchor block header not set after sync".into()))
    }

    /// Get the current anchor block number.
    async fn get_anchor_block_number(&self) -> Result<u64, Error> {
        self.sync_block_state().await?;
        self.block_synchronizer.get_anchor_block_number().await
    }

    /// Get a reference to the underlying node client.
    pub fn node(&self) -> &N {
        &self.node
    }

    /// Get a reference to the contract store.
    pub fn contract_store(&self) -> &ContractStore {
        &self.contract_store
    }

    /// Get a reference to the key store.
    pub fn key_store(&self) -> &KeyStore {
        &self.key_store
    }

    /// Get a reference to the address store.
    pub fn address_store(&self) -> &AddressStore {
        &self.address_store
    }

    /// Get a reference to the note store.
    pub fn note_store(&self) -> &NoteStore {
        &self.note_store
    }

    /// Get a reference to the anchor block store.
    pub fn anchor_block_store(&self) -> &AnchorBlockStore {
        &self.anchor_block_store
    }

    /// Get a reference to the private event store.
    pub fn private_event_store(&self) -> &PrivateEventStore {
        &self.private_event_store
    }

    /// Extract the target contract address, function name, encoded args, and origin
    /// from a serialized TxExecutionRequest.
    ///
    /// The TxExecutionRequest is built by the account entrypoint and contains:
    /// - `origin`: the account address (msg_sender)
    /// - Encoded calls inside `args_of_calls` with target addresses
    ///
    /// We try multiple strategies to find the target contract:
    /// 1. Direct `contractAddress` / `to` field (simple format)
    /// 2. Extract from encoded calls (entrypoint format)
    /// 3. Scan all registered contracts for matching function
    #[allow(dead_code)]
    async fn extract_call_info(
        &self,
        tx_request: &TxExecutionRequest,
    ) -> Result<(AztecAddress, String, Vec<Fr>, AztecAddress), Error> {
        let request: DecodedTxExecutionRequest = serde_json::from_value(tx_request.data.clone())?;
        let origin = request.origin;

        if let Some(call) = Self::decode_entrypoint_call(&request)? {
            let function_name = self
                .resolve_function_name_by_selector(&call.to, call.selector)
                .await;
            return Ok((call.to, function_name, call.encoded_args, origin));
        }

        // Fallback: scan registered contracts
        let contracts = self.contract_store.get_contract_addresses().await?;
        if let Some(addr) = contracts.first() {
            return Ok((*addr, "unknown".to_owned(), vec![], origin));
        }

        Err(Error::InvalidData(
            "could not determine target contract from TxExecutionRequest".into(),
        ))
    }

    /// Resolve function name from contract address and selector hex string.
    async fn resolve_function_name_by_selector(
        &self,
        addr: &AztecAddress,
        sel: aztec_core::abi::FunctionSelector,
    ) -> String {
        if let Some(name) = Self::resolve_protocol_function_name(addr, sel) {
            return name.to_owned();
        }

        let inst = match self.contract_store.get_instance(addr).await {
            Ok(Some(i)) => i,
            _ => return "unknown".to_owned(),
        };
        let artifact = match self
            .contract_store
            .get_artifact(&inst.inner.current_contract_class_id)
            .await
        {
            Ok(Some(a)) => a,
            _ => return "unknown".to_owned(),
        };
        artifact
            .find_function_by_selector(&sel)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "unknown".to_owned())
    }

    fn resolve_protocol_function_name(
        addr: &AztecAddress,
        sel: aztec_core::abi::FunctionSelector,
    ) -> Option<&'static str> {
        if *addr == protocol_contract_address::contract_class_registry()
            && sel
                == aztec_core::abi::FunctionSelector::from_signature("publish(Field,Field,Field)")
        {
            return Some("publish");
        }

        if *addr == protocol_contract_address::contract_instance_registry()
            && sel
                == aztec_core::abi::FunctionSelector::from_signature(
                    "publish_for_public_execution(Field,(Field),Field,(((Field,Field,bool)),((Field,Field,bool)),((Field,Field,bool)),((Field,Field,bool))),bool)",
                )
        {
            return Some("publish_for_public_execution");
        }

        // AuthRegistry protocol contract
        if *addr == protocol_contract_address::auth_registry() {
            if sel
                == aztec_core::abi::FunctionSelector::from_signature("set_authorized(Field,bool)")
            {
                return Some("set_authorized(Field,bool)");
            }
            if sel == aztec_core::abi::FunctionSelector::from_signature("consume((Field),Field)") {
                return Some("consume((Field),Field)");
            }
        }

        None
    }

    fn protocol_private_execution(
        &self,
        tx_request: &TxExecutionRequest,
        contract_address: AztecAddress,
        function_name: &str,
        encoded_args: &[Fr],
        origin: AztecAddress,
        first_nullifier: Fr,
    ) -> Result<Option<CallExecutionBundle>, Error> {
        match (contract_address, function_name) {
            (addr, "publish") if addr == protocol_contract_address::contract_class_registry() => {
                let artifact_hash = *encoded_args
                    .first()
                    .ok_or_else(|| Error::InvalidData("publish missing artifact_hash".into()))?;
                let private_functions_root = *encoded_args.get(1).ok_or_else(|| {
                    Error::InvalidData("publish missing private_functions_root".into())
                })?;
                let public_bytecode_commitment = *encoded_args.get(2).ok_or_else(|| {
                    Error::InvalidData("publish missing public_bytecode_commitment".into())
                })?;
                let class_id = compute_contract_class_id(
                    artifact_hash,
                    private_functions_root,
                    public_bytecode_commitment,
                );

                let capsules = tx_request
                    .data
                    .get("capsules")
                    .cloned()
                    .map(serde_json::from_value::<Vec<Capsule>>)
                    .transpose()?
                    .unwrap_or_default();
                let bytecode_fields = capsules
                    .into_iter()
                    .find(|capsule| {
                        capsule.contract_address == protocol_contract_address::contract_class_registry()
                            && capsule.storage_slot
                                == aztec_core::constants::contract_class_registry_bytecode_capsule_slot()
                    })
                    .map(|capsule| capsule.data)
                    .unwrap_or_default();

                let mut emitted_fields =
                    Vec::with_capacity(MAX_PACKED_PUBLIC_BYTECODE_SIZE_IN_FIELDS + 5);
                emitted_fields.push(contract_class_published_magic_value());
                emitted_fields.push(class_id);
                emitted_fields.push(Fr::from(1u64));
                emitted_fields.push(artifact_hash);
                emitted_fields.push(private_functions_root);
                emitted_fields.extend(bytecode_fields);
                let entrypoint = crate::execution::execution_result::PrivateCallExecutionResult {
                    contract_address,
                    call_context: aztec_core::kernel_types::CallContext {
                        msg_sender: origin,
                        contract_address,
                        function_selector: FunctionSelector::from_signature(
                            "publish(Field,Field,Field)",
                        )
                        .to_field(),
                        is_static_call: false,
                    },
                    nullifiers: vec![aztec_core::kernel_types::ScopedNullifier {
                        nullifier: aztec_core::kernel_types::Nullifier {
                            value: class_id,
                            note_hash: Fr::zero(),
                            counter: 2,
                        },
                        contract_address,
                    }],
                    contract_class_logs: vec![aztec_core::kernel_types::CountedContractClassLog {
                        log: aztec_core::kernel_types::ContractClassLog {
                            contract_address,
                            emitted_length: emitted_fields.len() as u32,
                            fields: emitted_fields.clone(),
                        },
                        counter: 3,
                    }],
                    start_side_effect_counter: 2,
                    end_side_effect_counter: 4,
                    min_revertible_side_effect_counter: 2,
                    ..Default::default()
                };

                return Ok(Some(CallExecutionBundle {
                    first_acir_call_return_values: Vec::new(),
                    simulated_return_values: Vec::new(),
                    execution_result: crate::execution::execution_result::PrivateExecutionResult {
                        entrypoint,
                        first_nullifier,
                        expiration_timestamp: 0,
                        public_function_calldata: vec![],
                    },
                    contract_class_log_fields: vec![
                        aztec_core::tx::ContractClassLogFields::from_emitted_fields(emitted_fields),
                    ],
                    public_function_calldata: vec![],
                }));
            }
            (addr, "publish_for_public_execution")
                if addr == protocol_contract_address::contract_instance_registry() =>
            {
                if encoded_args.len() < 16 {
                    return Err(Error::InvalidData(format!(
                        "publish_for_public_execution args too short: {}",
                        encoded_args.len()
                    )));
                }

                let salt = encoded_args[0];
                let class_id = encoded_args[1];
                let initialization_hash = encoded_args[2];
                let public_keys = PublicKeys {
                    master_nullifier_public_key: Point {
                        x: encoded_args[3],
                        y: encoded_args[4],
                        is_infinite: encoded_args[5] != Fr::zero(),
                    },
                    master_incoming_viewing_public_key: Point {
                        x: encoded_args[6],
                        y: encoded_args[7],
                        is_infinite: encoded_args[8] != Fr::zero(),
                    },
                    master_outgoing_viewing_public_key: Point {
                        x: encoded_args[9],
                        y: encoded_args[10],
                        is_infinite: encoded_args[11] != Fr::zero(),
                    },
                    master_tagging_public_key: Point {
                        x: encoded_args[12],
                        y: encoded_args[13],
                        is_infinite: encoded_args[14] != Fr::zero(),
                    },
                };
                let universal_deploy = encoded_args[15] != Fr::zero();
                let deployer = if universal_deploy {
                    AztecAddress::zero()
                } else {
                    origin
                };
                let instance = ContractInstanceWithAddress {
                    address: compute_contract_address_from_instance(&ContractInstance {
                        version: 1,
                        salt,
                        deployer,
                        current_contract_class_id: class_id,
                        original_contract_class_id: class_id,
                        initialization_hash,
                        public_keys: public_keys.clone(),
                    })?,
                    inner: ContractInstance {
                        version: 1,
                        salt,
                        deployer,
                        current_contract_class_id: class_id,
                        original_contract_class_id: class_id,
                        initialization_hash,
                        public_keys: public_keys.clone(),
                    },
                };

                let event_payload = vec![
                    contract_instance_published_magic_value(),
                    instance.address.0,
                    Fr::from(1u64),
                    salt,
                    class_id,
                    initialization_hash,
                    public_keys.master_nullifier_public_key.x,
                    public_keys.master_nullifier_public_key.y,
                    public_keys.master_incoming_viewing_public_key.x,
                    public_keys.master_incoming_viewing_public_key.y,
                    public_keys.master_outgoing_viewing_public_key.x,
                    public_keys.master_outgoing_viewing_public_key.y,
                    public_keys.master_tagging_public_key.x,
                    public_keys.master_tagging_public_key.y,
                    deployer.0,
                ];
                let mut emitted_private_log_fields = event_payload.clone();
                emitted_private_log_fields.push(Fr::zero());
                // Emit the raw address as the nullifier — the kernel silos it later
                // with the deployer protocol contract address.
                let entrypoint = crate::execution::execution_result::PrivateCallExecutionResult {
                    contract_address,
                    call_context: aztec_core::kernel_types::CallContext {
                        msg_sender: origin,
                        contract_address,
                        function_selector: FunctionSelector::from_signature(
                            "publish_for_public_execution(Field,(Field),Field,(((Field,Field,bool)),((Field,Field,bool)),((Field,Field,bool)),((Field,Field,bool))),bool)",
                        )
                        .to_field(),
                        is_static_call: false,
                    },
                    nullifiers: vec![aztec_core::kernel_types::ScopedNullifier {
                        nullifier: aztec_core::kernel_types::Nullifier {
                            value: instance.address.0,
                            note_hash: Fr::zero(),
                            counter: 2,
                        },
                        contract_address,
                    }],
                    private_logs: vec![crate::execution::execution_result::PrivateLogData {
                        fields: emitted_private_log_fields,
                        emitted_length: 15,
                        note_hash_counter: 0,
                        counter: 3,
                        contract_address,
                    }],
                    start_side_effect_counter: 2,
                    end_side_effect_counter: 4,
                    min_revertible_side_effect_counter: 2,
                    ..Default::default()
                };

                return Ok(Some(CallExecutionBundle {
                    first_acir_call_return_values: Vec::new(),
                    simulated_return_values: Vec::new(),
                    execution_result: crate::execution::execution_result::PrivateExecutionResult {
                        entrypoint,
                        first_nullifier,
                        expiration_timestamp: 0,
                        public_function_calldata: vec![],
                    },
                    contract_class_log_fields: vec![],
                    public_function_calldata: vec![],
                }));
            }
            _ => {}
        }

        Ok(None)
    }

    /// Handle public function calls to protocol contracts that may not be
    /// registered in the local contract store (e.g. AuthRegistry).
    ///
    /// For public calls the PXE only needs to build a `PublicCallRequestData`;
    /// it does not execute the function — that happens on the sequencer.
    fn protocol_public_execution(
        contract_address: AztecAddress,
        function_name: &str,
        encoded_args: &[Fr],
        origin: AztecAddress,
        first_nullifier: Fr,
        hide_msg_sender: bool,
        is_static_call: bool,
    ) -> Result<Option<CallExecutionBundle>, Error> {
        use crate::execution::execution_result::{
            PrivateCallExecutionResult, PrivateExecutionResult, PublicCallRequestData,
        };

        // Only handle known protocol contract addresses.
        if contract_address != protocol_contract_address::auth_registry() {
            return Ok(None);
        }

        // Compute selector from function name.
        let selector_fr: Fr = FunctionSelector::from_signature(function_name).into();
        let mut calldata = vec![selector_fr];
        calldata.extend_from_slice(encoded_args);
        let hashed = aztec_core::tx::HashedValues::from_calldata(calldata);
        let calldata_hash = hashed.hash();
        let msg_sender = if hide_msg_sender {
            AztecAddress::zero()
        } else {
            origin
        };

        let entrypoint = PrivateCallExecutionResult {
            contract_address: origin,
            public_call_requests: vec![PublicCallRequestData {
                contract_address,
                msg_sender,
                is_static_call,
                calldata_hash,
                counter: 2,
            }],
            start_side_effect_counter: 2,
            min_revertible_side_effect_counter: 2,
            end_side_effect_counter: 3,
            ..Default::default()
        };

        let exec_result = PrivateExecutionResult {
            entrypoint,
            first_nullifier,
            expiration_timestamp: 0,
            public_function_calldata: vec![hashed.clone()],
        };

        Ok(Some(CallExecutionBundle {
            first_acir_call_return_values: Vec::new(),
            simulated_return_values: Vec::new(),
            execution_result: exec_result,
            contract_class_log_fields: vec![],
            public_function_calldata: vec![hashed],
        }))
    }

    #[allow(dead_code)]
    fn decode_entrypoint_call(
        request: &DecodedTxExecutionRequest,
    ) -> Result<Option<DecodedEntrypointCall>, Error> {
        Ok(Self::decode_entrypoint_calls(request)?.into_iter().next())
    }

    fn decode_entrypoint_calls(
        request: &DecodedTxExecutionRequest,
    ) -> Result<Vec<DecodedEntrypointCall>, Error> {
        let entrypoint_args = request
            .args_of_calls
            .iter()
            .find(|hv| hv.hash == request.first_call_args_hash)
            .ok_or_else(|| {
                Error::InvalidData("firstCallArgsHash not found in argsOfCalls".into())
            })?;

        let encoded_calls = parse_encoded_calls(&entrypoint_args.values)?;
        let mut decoded = Vec::new();
        for call in encoded_calls {
            if call.to == AztecAddress::zero()
                || call.selector == aztec_core::abi::FunctionSelector::empty()
            {
                continue;
            }

            let hashed_args = request
                .args_of_calls
                .iter()
                .find(|hv| hv.hash == call.args_hash)
                .ok_or_else(|| {
                    Error::InvalidData(format!(
                        "call args hash {} not found in argsOfCalls",
                        call.args_hash
                    ))
                })?;

            let encoded_args = if call.is_public {
                hashed_args.values.iter().copied().skip(1).collect()
            } else {
                hashed_args.values.clone()
            };

            decoded.push(DecodedEntrypointCall {
                to: call.to,
                selector: call.selector,
                encoded_args,
                hide_msg_sender: call.hide_msg_sender,
                is_static: call.is_static,
            });
        }

        Ok(decoded)
    }

    /// Extract args from tx request data.
    #[allow(dead_code)] // Used when ACVM integration is complete
    fn extract_args(data: &serde_json::Value) -> Vec<Fr> {
        data.get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().and_then(|s| Fr::from_hex(s).ok()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Build the full initial witness for a private function call.
    ///
    /// Private functions expect `[PrivateContextInputs fields..., user args...]`
    /// where PrivateContextInputs is 37 fields: call_context(4) +
    /// anchor_block_header(22) + tx_context(10) + start_side_effect_counter(1).
    fn build_private_witness(
        &self,
        artifact: &ContractArtifact,
        function_name: &str,
        user_args: &[Fr],
        contract_address: AztecAddress,
        msg_sender: AztecAddress,
        tx_request: &TxExecutionRequest,
        anchor: &AnchorBlockHeader,
        function_selector: aztec_core::abi::FunctionSelector,
        is_static_call: bool,
    ) -> Vec<Fr> {
        let context_size = artifact.private_context_inputs_size(function_name);
        if context_size == 0 {
            // Not a private function or no context inputs needed
            return user_args.to_vec();
        }

        let mut witness = Vec::with_capacity(context_size + user_args.len());
        let tx_constants = Self::build_tx_constant_data(
            anchor,
            tx_request,
            self.vk_tree_root,
            self.protocol_contracts_hash,
        );
        let call_context = aztec_core::kernel_types::CallContext {
            msg_sender,
            contract_address,
            function_selector: function_selector.to_field(),
            is_static_call,
        };

        witness.extend(call_context.to_fields());
        witness.extend(tx_constants.anchor_block_header.to_fields());
        witness.extend(tx_constants.tx_context.to_fields());
        // Upstream reserves the first side effect slot for the tx hash.
        witness.push(Fr::from(2u64));

        // Ensure we have exactly context_size fields
        witness.truncate(context_size);
        while witness.len() < context_size {
            witness.push(Fr::zero());
        }

        // Append user args
        witness.extend_from_slice(user_args);

        witness
    }

    fn offset_private_call_result(
        call: &crate::execution::execution_result::PrivateCallExecutionResult,
        offset: u32,
    ) -> crate::execution::execution_result::PrivateCallExecutionResult {
        let mut adjusted = call.clone();
        adjusted.start_side_effect_counter =
            adjusted.start_side_effect_counter.saturating_add(offset);
        adjusted.end_side_effect_counter = adjusted.end_side_effect_counter.saturating_add(offset);
        adjusted.min_revertible_side_effect_counter = adjusted
            .min_revertible_side_effect_counter
            .saturating_add(offset);

        for note in &mut adjusted.new_notes {
            note.counter = note.counter.saturating_add(offset);
        }
        adjusted.note_hash_nullifier_counter_map = adjusted
            .note_hash_nullifier_counter_map
            .iter()
            .map(|(note_counter, nullifier_counter)| {
                (
                    note_counter.saturating_add(offset),
                    nullifier_counter.saturating_add(offset),
                )
            })
            .collect();
        for log in &mut adjusted.contract_class_logs {
            log.counter = log.counter.saturating_add(offset);
        }
        for note_hash in &mut adjusted.note_hashes {
            note_hash.note_hash.counter = note_hash.note_hash.counter.saturating_add(offset);
        }
        for nullifier in &mut adjusted.nullifiers {
            nullifier.nullifier.counter = nullifier.nullifier.counter.saturating_add(offset);
        }
        for req in &mut adjusted.note_hash_read_requests {
            req.read_request.counter = req.read_request.counter.saturating_add(offset);
        }
        for req in &mut adjusted.nullifier_read_requests {
            req.read_request.counter = req.read_request.counter.saturating_add(offset);
        }
        for log in &mut adjusted.private_logs {
            log.counter = log.counter.saturating_add(offset);
        }
        for req in &mut adjusted.public_call_requests {
            req.counter = req.counter.saturating_add(offset);
        }
        if let Some(req) = &mut adjusted.public_teardown_call_request {
            req.counter = req.counter.saturating_add(offset);
        }
        adjusted.nested_execution_results = adjusted
            .nested_execution_results
            .iter()
            .map(|nested| Self::offset_private_call_result(nested, offset))
            .collect();
        adjusted
    }

    fn aggregate_call_bundles(
        origin: AztecAddress,
        bundles: Vec<CallExecutionBundle>,
    ) -> CallExecutionBundle {
        let mut offset = 0u32;
        let mut nested_execution_results = Vec::with_capacity(bundles.len());
        let mut first_nullifier = Fr::zero();
        let mut public_function_calldata = Vec::new();
        let mut contract_class_log_fields = Vec::new();
        let mut min_revertible_side_effect_counter = 0u32;
        let mut first_acir_returns = Vec::new();
        let mut simulated_return_values = Vec::new();
        let mut expiration_timestamp = 0u64;

        for (idx, bundle) in bundles.into_iter().enumerate() {
            if idx == 0 {
                first_nullifier = bundle.execution_result.first_nullifier;
                first_acir_returns = bundle.first_acir_call_return_values;
                simulated_return_values = bundle.simulated_return_values;
                expiration_timestamp = bundle.execution_result.expiration_timestamp;
                min_revertible_side_effect_counter = bundle
                    .execution_result
                    .entrypoint
                    .min_revertible_side_effect_counter;
            } else if bundle.execution_result.expiration_timestamp != 0 {
                expiration_timestamp = if expiration_timestamp == 0 {
                    bundle.execution_result.expiration_timestamp
                } else {
                    expiration_timestamp.min(bundle.execution_result.expiration_timestamp)
                };
            }
            let adjusted_entrypoint =
                Self::offset_private_call_result(&bundle.execution_result.entrypoint, offset);
            offset = adjusted_entrypoint.end_side_effect_counter;
            nested_execution_results.push(adjusted_entrypoint);
            public_function_calldata.extend(bundle.public_function_calldata);
            contract_class_log_fields.extend(bundle.contract_class_log_fields);
        }

        let root = crate::execution::execution_result::PrivateCallExecutionResult {
            contract_address: origin,
            call_context: aztec_core::kernel_types::CallContext {
                msg_sender: origin,
                contract_address: origin,
                function_selector: Fr::zero(),
                is_static_call: false,
            },
            nested_execution_results,
            start_side_effect_counter: 0,
            end_side_effect_counter: offset,
            min_revertible_side_effect_counter,
            ..Default::default()
        };

        CallExecutionBundle {
            first_acir_call_return_values: first_acir_returns,
            simulated_return_values,
            execution_result: crate::execution::execution_result::PrivateExecutionResult {
                entrypoint: root,
                first_nullifier,
                expiration_timestamp,
                public_function_calldata: public_function_calldata.clone(),
            },
            contract_class_log_fields,
            public_function_calldata,
        }
    }

    async fn execute_entrypoint_call_bundle(
        &self,
        tx_request: &TxExecutionRequest,
        call: &DecodedEntrypointCall,
        origin: AztecAddress,
        protocol_nullifier: Fr,
        anchor: &AnchorBlockHeader,
        scopes: &[AztecAddress],
    ) -> Result<CallExecutionBundle, Error> {
        let contract_address = call.to;
        let function_name = self
            .resolve_function_name_by_selector(&contract_address, call.selector)
            .await;

        if let Some(bundle) = self.protocol_private_execution(
            tx_request,
            contract_address,
            &function_name,
            &call.encoded_args,
            origin,
            protocol_nullifier,
        )? {
            return Ok(bundle);
        }

        // Handle public calls to protocol contracts (e.g. AuthRegistry)
        // that may not be registered in the local contract store.
        if let Some(bundle) = Self::protocol_public_execution(
            contract_address,
            &function_name,
            &call.encoded_args,
            origin,
            protocol_nullifier,
            call.hide_msg_sender,
            call.is_static,
        )? {
            return Ok(bundle);
        }

        let contract_instance = self.contract_store.get_instance(&contract_address).await?;
        let class_id = contract_instance
            .as_ref()
            .map(|i| i.inner.current_contract_class_id)
            .ok_or_else(|| Error::InvalidData(format!("contract not found: {contract_address}")))?;
        let artifact = self
            .contract_store
            .get_artifact(&class_id)
            .await?
            .ok_or_else(|| {
                Error::InvalidData(format!("artifact not found for class {class_id}"))
            })?;
        let function = artifact.find_function(&function_name)?;

        if function.function_type == FunctionType::Public {
            let (execution_result, contract_class_log_fields, public_function_calldata) =
                Self::build_public_call_execution(
                    &artifact,
                    &function_name,
                    &call.encoded_args,
                    contract_address,
                    origin,
                    protocol_nullifier,
                    call.hide_msg_sender,
                    call.is_static,
                )?;
            return Ok(CallExecutionBundle {
                first_acir_call_return_values: Vec::new(),
                simulated_return_values: Vec::new(),
                execution_result,
                contract_class_log_fields,
                public_function_calldata,
            });
        }

        let full_witness = self.build_private_witness(
            &artifact,
            &function_name,
            &call.encoded_args,
            contract_address,
            origin,
            tx_request,
            anchor,
            function.selector.expect("private function selector"),
            function.is_static,
        );

        let mut oracle = crate::execution::PrivateExecutionOracle::new(
            &self.node,
            &self.contract_store,
            &self.key_store,
            &self.note_store,
            &self.capsule_store,
            &self.address_store,
            &self.sender_tagging_store,
            anchor.data.clone(),
            contract_address,
            protocol_nullifier,
            Some(origin),
            scopes.to_vec(),
            call.is_static,
        );

        // Extract and set auth witnesses from the TX request so nested
        // calls (e.g. verify_private_authwit) can look up the signature.
        if let Some(auth_witnesses) = tx_request.data.get("authWitnesses").and_then(|v| {
            serde_json::from_value::<Vec<aztec_core::tx::AuthWitness>>(v.clone()).ok()
        }) {
            let pairs: Vec<(Fr, Vec<Fr>)> = auth_witnesses
                .iter()
                .map(|aw| (aw.request_hash, aw.fields.clone()))
                .collect();
            oracle.set_auth_witnesses(pairs);
        }

        // Store the block-header + tx-context portion of the witness so
        // nested calls can reuse it (for chain_id, version, etc.).
        let context_inputs_size = artifact.private_context_inputs_size(&function_name);
        if context_inputs_size > 5 {
            // Skip call_context (4 fields), take everything up to (but not
            // including) the last field (start_side_effect_counter).
            oracle.context_witness_prefix =
                full_witness[4..context_inputs_size.saturating_sub(1)].to_vec();
        }

        let acvm_output = crate::execution::AcvmExecutor::execute_private(
            &artifact,
            &function_name,
            &full_witness,
            &mut oracle,
        )
        .await?;

        // Extract return values from the execution cache (for databus returns).
        // The PCPI layout: call_context(4), args_hash(1), returns_hash(1), ...
        // returns_hash is at offset 5 from the PCPI start.
        let acir_call_returns = {
            let ctx_size = artifact.private_context_inputs_size(&function_name);
            let user_args_size = call.encoded_args.len();
            let pcpi_start = ctx_size + user_args_size;
            const PCPI_RETURNS_HASH_OFFSET: usize = 5;
            let returns_hash_idx =
                acir::native_types::Witness((pcpi_start + PCPI_RETURNS_HASH_OFFSET) as u32);
            let returns_hash = acvm_output
                .witness
                .get(&returns_hash_idx)
                .map(super::execution::field_conversion::fe_to_fr);
            if let Some(rh) = returns_hash {
                oracle.get_execution_cache_entry(&rh).unwrap_or_default()
            } else {
                acvm_output.first_acir_call_return_values.clone()
            }
        };
        let mut execution_result = oracle.build_execution_result(acvm_output, contract_address, 0);
        execution_result.entrypoint.call_context = aztec_core::kernel_types::CallContext {
            msg_sender: origin,
            contract_address,
            function_selector: function
                .selector
                .expect("private function selector")
                .to_field(),
            is_static_call: function.is_static,
        };

        // Extract circuit-constrained side effects from PrivateCircuitPublicInputs
        // in the solved witness. These are NOT emitted through oracle calls.
        {
            let ctx_size = artifact.private_context_inputs_size(&function_name);
            let user_args_size = call.encoded_args.len();
            let params_size = ctx_size + user_args_size;
            let expiration_timestamp = Self::extract_expiration_timestamp_from_witness(
                &execution_result.entrypoint.partial_witness,
                params_size,
                ctx_size,
            );
            execution_result.expiration_timestamp = expiration_timestamp;

            let (circuit_note_hashes, _circuit_nullifiers, circuit_logs) =
                Self::extract_side_effects_from_witness(
                    &execution_result.entrypoint.partial_witness,
                    params_size,
                    contract_address,
                );
            // Note hashes may come from oracle calls (notifyCreatedNote)
            // OR from the PCPI witness. Only add PCPI note hashes if the
            // oracle didn't produce any, to avoid duplicates.
            if execution_result.entrypoint.note_hashes.is_empty() && !circuit_note_hashes.is_empty()
            {
                execution_result
                    .entrypoint
                    .note_hashes
                    .extend(circuit_note_hashes);
            }
            // Private logs are always circuit-constrained (never from oracle).
            if !circuit_logs.is_empty() {
                execution_result
                    .entrypoint
                    .private_logs
                    .extend(circuit_logs);
            }
        }

        self.persist_pending_notes(&execution_result, scopes)
            .await?;

        let contract_class_log_fields = execution_result
            .all_contract_class_logs_sorted()
            .iter()
            .map(|ccl| {
                aztec_core::tx::ContractClassLogFields::from_emitted_fields(ccl.log.fields.clone())
            })
            .collect::<Vec<_>>();
        let public_function_calldata = execution_result.public_function_calldata.clone();

        let simulated_return_values = if !acir_call_returns.is_empty() {
            acir_call_returns.clone()
        } else {
            execution_result.entrypoint.return_values.clone()
        };

        Ok(CallExecutionBundle {
            execution_result,
            contract_class_log_fields,
            public_function_calldata,
            first_acir_call_return_values: acir_call_returns,
            simulated_return_values,
        })
    }

    /// Extract the origin (msg_sender) from a TxExecutionRequest.
    /// Extract all circuit-constrained side effects from the solved ACVM
    /// witness (`PrivateCircuitPublicInputs`, 870 fields starting at
    /// witness index `params_size`).
    ///
    /// Returns (note_hashes, nullifiers, private_logs).
    fn extract_side_effects_from_witness(
        witness: &acir::native_types::WitnessMap<acir::FieldElement>,
        params_size: usize,
        contract_address: AztecAddress,
    ) -> (
        Vec<aztec_core::kernel_types::ScopedNoteHash>,
        Vec<aztec_core::kernel_types::ScopedNullifier>,
        Vec<crate::execution::PrivateLogData>,
    ) {
        use aztec_core::kernel_types::{NoteHash, Nullifier, ScopedNoteHash, ScopedNullifier};

        const PCPI_LENGTH: usize = 870;
        const NOTE_HASHES_OFFSET: usize = 454;
        const NOTE_HASH_LEN: usize = 2;
        const MAX_NOTE_HASHES: usize = 16;
        const NOTE_HASHES_ARRAY_LEN: usize = MAX_NOTE_HASHES * NOTE_HASH_LEN + 1;
        const NULLIFIERS_OFFSET: usize = 487;
        const NULLIFIER_LEN: usize = 3;
        const MAX_NULLIFIERS: usize = 16;
        const NULLIFIERS_ARRAY_LEN: usize = MAX_NULLIFIERS * NULLIFIER_LEN + 1;
        const PRIVATE_LOGS_OFFSET: usize = 561;
        const PRIVATE_LOG_DATA_LEN: usize = 19;
        const PRIVATE_LOG_FIELDS: usize = 16;
        const MAX_LOGS: usize = 16;
        const PRIVATE_LOGS_ARRAY_LEN: usize = MAX_LOGS * PRIVATE_LOG_DATA_LEN + 1;

        let pcpi_start = params_size;
        let mut pcpi = Vec::with_capacity(PCPI_LENGTH);
        for i in 0..PCPI_LENGTH {
            let idx = acir::native_types::Witness((pcpi_start + i) as u32);
            let val = witness
                .get(&idx)
                .map(|fe| crate::execution::field_conversion::fe_to_fr(fe))
                .unwrap_or_else(Fr::zero);
            pcpi.push(val);
        }

        // Extract note hashes
        let nh_slice = &pcpi[NOTE_HASHES_OFFSET..][..NOTE_HASHES_ARRAY_LEN];
        let nh_count = nh_slice[NOTE_HASHES_ARRAY_LEN - 1]
            .to_usize()
            .min(MAX_NOTE_HASHES);
        let mut note_hashes = Vec::with_capacity(nh_count);
        for i in 0..nh_count {
            let base = i * NOTE_HASH_LEN;
            let value = nh_slice[base];
            let counter = nh_slice[base + 1].to_usize() as u32;
            if value != Fr::zero() {
                note_hashes.push(ScopedNoteHash {
                    note_hash: NoteHash { value, counter },
                    contract_address,
                });
            }
        }

        // Extract nullifiers
        let null_slice = &pcpi[NULLIFIERS_OFFSET..][..NULLIFIERS_ARRAY_LEN];
        let null_count = null_slice[NULLIFIERS_ARRAY_LEN - 1]
            .to_usize()
            .min(MAX_NULLIFIERS);
        let mut nullifiers = Vec::with_capacity(null_count);
        for i in 0..null_count {
            let base = i * NULLIFIER_LEN;
            let value = null_slice[base];
            let note_hash = null_slice[base + 1];
            let counter = null_slice[base + 2].to_usize() as u32;
            if value != Fr::zero() {
                nullifiers.push(ScopedNullifier {
                    nullifier: Nullifier {
                        value,
                        note_hash,
                        counter,
                    },
                    contract_address,
                });
            }
        }

        // Extract private logs
        let logs_slice = &pcpi[PRIVATE_LOGS_OFFSET..][..PRIVATE_LOGS_ARRAY_LEN];
        let log_count = logs_slice[PRIVATE_LOGS_ARRAY_LEN - 1]
            .to_usize()
            .min(MAX_LOGS);
        let mut logs = Vec::with_capacity(log_count);
        for i in 0..log_count {
            let base = i * PRIVATE_LOG_DATA_LEN;
            let fields: Vec<Fr> = logs_slice[base..base + PRIVATE_LOG_FIELDS].to_vec();
            let emitted_length = logs_slice[base + PRIVATE_LOG_FIELDS].to_usize() as u32;
            let note_hash_counter = logs_slice[base + PRIVATE_LOG_FIELDS + 1].to_usize() as u32;
            let counter = logs_slice[base + PRIVATE_LOG_DATA_LEN - 1].to_usize() as u32;
            if emitted_length > 0 {
                logs.push(crate::execution::PrivateLogData {
                    fields,
                    emitted_length,
                    note_hash_counter,
                    counter,
                    contract_address,
                });
            }
        }

        (note_hashes, nullifiers, logs)
    }

    fn extract_expiration_timestamp_from_witness(
        witness: &acir::native_types::WitnessMap<acir::FieldElement>,
        params_size: usize,
        context_inputs_size: usize,
    ) -> u64 {
        let prefix_len = context_inputs_size.saturating_sub(5);
        let expiration_offset = prefix_len + 8;
        let idx = acir::native_types::Witness((params_size + expiration_offset) as u32);
        witness
            .get(&idx)
            .map(crate::execution::field_conversion::fe_to_fr)
            .map(|fr| fr.to_usize() as u64)
            .unwrap_or(0)
    }

    fn extract_origin(&self, tx_request: &TxExecutionRequest) -> AztecAddress {
        tx_request
            .data
            .get("origin")
            .and_then(|v| v.as_str())
            .and_then(|s| Fr::from_hex(s).ok())
            .map(AztecAddress)
            .unwrap_or(AztecAddress(Fr::zero()))
    }

    /// Verify auth witness Schnorr signatures against the origin account's
    /// stored signing public key.
    ///
    /// The Noir account contract's entrypoint verifies auth witnesses via
    /// circuit constraints, but our oracle-based execution skips those checks.
    /// This method replicates the verification so that `simulate_tx` (and
    /// `prove_tx`) reject transactions signed with an incorrect key.
    async fn verify_auth_witness_signatures(
        &self,
        tx_request: &TxExecutionRequest,
        origin: AztecAddress,
    ) -> Result<(), Error> {
        let auth_witnesses: Vec<aztec_core::tx::AuthWitness> = tx_request
            .data
            .get("authWitnesses")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        if auth_witnesses.is_empty() {
            return Ok(());
        }

        // Look up the signing public key note for the origin account.
        // Schnorr account contracts store the signing key at storage slot 1
        // as a two-field note [pk.x, pk.y].
        let signing_key_notes = self
            .note_store
            .get_notes_by_slot(&origin, &Fr::from(1u64))
            .await
            .unwrap_or_default();

        let signing_pk = signing_key_notes.iter().find_map(|note| {
            if note.note_data.len() >= 2 && !note.nullified {
                Some(Point {
                    x: note.note_data[0],
                    y: note.note_data[1],
                    is_infinite: false,
                })
            } else {
                None
            }
        });

        let Some(pk) = signing_pk else {
            // No signing key note found — skip verification (the account may
            // use a non-Schnorr scheme or the note hasn't been synced).
            return Ok(());
        };

        for aw in &auth_witnesses {
            // Schnorr signatures are exactly 64 fields (one per byte).
            if aw.fields.len() != 64 {
                continue;
            }

            let sig_bytes: Vec<u8> = aw.fields.iter().map(|f| f.to_usize() as u8).collect();
            let mut sig_arr = [0u8; 64];
            sig_arr.copy_from_slice(&sig_bytes);
            let sig = SchnorrSignature::from_bytes(&sig_arr);

            if !schnorr_verify(&pk, &aw.request_hash, &sig) {
                return Err(Error::InvalidData(
                    "Cannot satisfy constraint: auth witness signature verification failed".into(),
                ));
            }
        }

        Ok(())
    }

    /// Build `TxConstantData` from the anchor block header and tx request.
    fn build_tx_constant_data(
        anchor: &AnchorBlockHeader,
        tx_request: &TxExecutionRequest,
        vk_tree_root: Fr,
        protocol_contracts_hash: Fr,
    ) -> aztec_core::kernel_types::TxConstantData {
        let h = &anchor.data;

        // Helper to extract an Fr from a JSON path (hex string or integer).
        let fr_at = |val: &serde_json::Value, path: &str| -> Fr {
            let v = val.pointer(path);
            match v {
                Some(serde_json::Value::String(s)) => Fr::from_hex(s).unwrap_or(Fr::zero()),
                Some(serde_json::Value::Number(n)) => Fr::from(n.as_u64().unwrap_or(0)),
                _ => Fr::zero(),
            }
        };
        // Parse a string as hex (0x-prefixed) or decimal.
        let parse_u64_str = |s: &str| -> u64 {
            if let Some(hex) = s.strip_prefix("0x") {
                u64::from_str_radix(hex, 16).unwrap_or(0)
            } else {
                s.parse::<u64>().unwrap_or(0)
            }
        };
        let parse_u128_str = |s: &str| -> u128 {
            if let Some(hex) = s.strip_prefix("0x") {
                u128::from_str_radix(hex, 16).unwrap_or(0)
            } else {
                s.parse::<u128>().unwrap_or(0)
            }
        };
        let u32_at = |val: &serde_json::Value, path: &str| -> u32 {
            let v = val.pointer(path);
            match v {
                Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0) as u32,
                Some(serde_json::Value::String(s)) => parse_u64_str(s) as u32,
                _ => 0,
            }
        };
        let u64_at = |val: &serde_json::Value, path: &str| -> u64 {
            let v = val.pointer(path);
            match v {
                Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0),
                Some(serde_json::Value::String(s)) => parse_u64_str(s),
                _ => 0,
            }
        };
        let u128_at = |val: &serde_json::Value, path: &str| -> u128 {
            let v = val.pointer(path);
            match v {
                Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0) as u128,
                Some(serde_json::Value::String(s)) => parse_u128_str(s),
                _ => 0,
            }
        };
        let eth_at = |val: &serde_json::Value, path: &str| -> aztec_core::types::EthAddress {
            // Parse EthAddress from hex string — use Fr conversion which zero-pads correctly
            match val.pointer(path).and_then(|v| v.as_str()) {
                Some(s) => {
                    // Parse as Fr, then extract the low 20 bytes
                    let fr = Fr::from_hex(s).unwrap_or(Fr::zero());
                    let bytes = fr.to_be_bytes();
                    let mut addr = [0u8; 20];
                    addr.copy_from_slice(&bytes[12..32]);
                    aztec_core::types::EthAddress(addr)
                }
                None => aztec_core::types::EthAddress::default(),
            }
        };

        let snap = |val: &serde_json::Value,
                    prefix: &str|
         -> aztec_core::kernel_types::AppendOnlyTreeSnapshot {
            aztec_core::kernel_types::AppendOnlyTreeSnapshot {
                root: fr_at(val, &format!("{prefix}/root")),
                next_available_leaf_index: u32_at(val, &format!("{prefix}/nextAvailableLeafIndex")),
            }
        };

        let block_header = aztec_core::kernel_types::BlockHeader {
            last_archive: snap(h, "/lastArchive"),
            state: aztec_core::kernel_types::StateReference {
                l1_to_l2_message_tree: snap(h, "/state/l1ToL2MessageTree"),
                partial: aztec_core::kernel_types::PartialStateReference {
                    note_hash_tree: snap(h, "/state/partial/noteHashTree"),
                    nullifier_tree: snap(h, "/state/partial/nullifierTree"),
                    public_data_tree: snap(h, "/state/partial/publicDataTree"),
                },
            },
            sponge_blob_hash: fr_at(h, "/spongeBlobHash"),
            global_variables: aztec_core::kernel_types::GlobalVariables {
                chain_id: fr_at(h, "/globalVariables/chainId"),
                version: fr_at(h, "/globalVariables/version"),
                block_number: u64_at(h, "/globalVariables/blockNumber"),
                slot_number: u64_at(h, "/globalVariables/slotNumber"),
                timestamp: u64_at(h, "/globalVariables/timestamp"),
                coinbase: eth_at(h, "/globalVariables/coinbase"),
                fee_recipient: AztecAddress(fr_at(h, "/globalVariables/feeRecipient")),
                gas_fees: aztec_core::fee::GasFees {
                    fee_per_da_gas: u128_at(h, "/globalVariables/gasFees/feePerDaGas"),
                    fee_per_l2_gas: u128_at(h, "/globalVariables/gasFees/feePerL2Gas"),
                },
            },
            total_fees: fr_at(h, "/totalFees"),
            total_mana_used: fr_at(h, "/totalManaUsed"),
        };

        // Extract tx context from the request
        let req = &tx_request.data;
        let tx_context = aztec_core::kernel_types::TxContext {
            chain_id: fr_at(req, "/txContext/chainId"),
            version: fr_at(req, "/txContext/version"),
            gas_settings: aztec_core::fee::GasSettings {
                gas_limits: Some(aztec_core::fee::Gas {
                    da_gas: u64_at(req, "/txContext/gasSettings/gasLimits/daGas"),
                    l2_gas: u64_at(req, "/txContext/gasSettings/gasLimits/l2Gas"),
                }),
                teardown_gas_limits: Some(aztec_core::fee::Gas {
                    da_gas: u64_at(req, "/txContext/gasSettings/teardownGasLimits/daGas"),
                    l2_gas: u64_at(req, "/txContext/gasSettings/teardownGasLimits/l2Gas"),
                }),
                max_fee_per_gas: Some(aztec_core::fee::GasFees {
                    fee_per_da_gas: u128_at(req, "/txContext/gasSettings/maxFeePerGas/feePerDaGas"),
                    fee_per_l2_gas: u128_at(req, "/txContext/gasSettings/maxFeePerGas/feePerL2Gas"),
                }),
                max_priority_fee_per_gas: Some(aztec_core::fee::GasFees {
                    fee_per_da_gas: u128_at(
                        req,
                        "/txContext/gasSettings/maxPriorityFeePerGas/feePerDaGas",
                    ),
                    fee_per_l2_gas: u128_at(
                        req,
                        "/txContext/gasSettings/maxPriorityFeePerGas/feePerL2Gas",
                    ),
                }),
            },
        };

        aztec_core::kernel_types::TxConstantData {
            anchor_block_header: block_header,
            tx_context,
            vk_tree_root,
            protocol_contracts_hash,
        }
    }

    /// Compute the expiration timestamp from the anchor block header.
    fn compute_expiration(anchor: &AnchorBlockHeader) -> u64 {
        let timestamp = anchor
            .data
            .pointer("/globalVariables/timestamp")
            .and_then(|v| {
                v.as_u64().or_else(|| {
                    v.as_str().and_then(|s| {
                        if let Some(hex) = s.strip_prefix("0x") {
                            u64::from_str_radix(hex, 16).ok()
                        } else {
                            s.parse::<u64>().ok()
                        }
                    })
                })
            })
            .unwrap_or(0);
        timestamp + aztec_core::constants::MAX_TX_LIFETIME
    }

    /// Ensure the tx's max_fee_per_gas is at least the current network gas price.
    ///
    /// Reads the current gas fees from the anchor block header and applies a 1.5x
    /// safety multiplier. If the tx's fee cap is below this, it is raised.
    fn ensure_min_fees(
        tx_constants: &mut aztec_core::kernel_types::TxConstantData,
        anchor: &AnchorBlockHeader,
    ) {
        let parse_u128 = |val: &serde_json::Value| -> u128 {
            if let Some(s) = val.as_str() {
                let s = s.strip_prefix("0x").unwrap_or(s);
                u128::from_str_radix(
                    s,
                    if val.as_str().unwrap_or("").starts_with("0x") {
                        16
                    } else {
                        10
                    },
                )
                .unwrap_or(0)
            } else {
                val.as_u64().unwrap_or(0) as u128
            }
        };

        let block_da_fee = anchor
            .data
            .pointer("/globalVariables/gasFees/feePerDaGas")
            .map(|v| parse_u128(v))
            .unwrap_or(0);
        let block_l2_fee = anchor
            .data
            .pointer("/globalVariables/gasFees/feePerL2Gas")
            .map(|v| parse_u128(v))
            .unwrap_or(0);

        // Apply 1.5x safety margin
        let min_da = block_da_fee + block_da_fee / 2;
        let min_l2 = block_l2_fee + block_l2_fee / 2;

        if let Some(ref mut fees) = tx_constants.tx_context.gas_settings.max_fee_per_gas {
            if fees.fee_per_da_gas < min_da {
                fees.fee_per_da_gas = min_da;
            }
            if fees.fee_per_l2_gas < min_l2 {
                fees.fee_per_l2_gas = min_l2;
            }
        } else {
            tx_constants.tx_context.gas_settings.max_fee_per_gas = Some(aztec_core::fee::GasFees {
                fee_per_da_gas: min_da,
                fee_per_l2_gas: min_l2,
            });
        }
    }

    /// Build a synthetic execution result for a public function call.
    ///
    /// Run the account contract's entrypoint function through ACVM.
    ///
    /// This mirrors the TS SDK flow where the entire entrypoint Noir circuit is
    /// executed. The circuit handles both private (via nested ACVM calls) and
    /// public (via `enqueuePublicFunctionCall` oracle) calls. This produces
    /// correct `PublicCallRequest` data that the node can process.
    async fn execute_entrypoint_via_acvm(
        &self,
        tx_request: &TxExecutionRequest,
        origin: AztecAddress,
        protocol_nullifier: Fr,
        anchor: &AnchorBlockHeader,
        scopes: &[AztecAddress],
    ) -> Result<CallExecutionBundle, Error> {
        let request: DecodedTxExecutionRequest = serde_json::from_value(tx_request.data.clone())?;

        // Find the account contract artifact
        let contract_instance = self.contract_store.get_instance(&origin).await?;
        let class_id = contract_instance
            .as_ref()
            .map(|i| i.inner.current_contract_class_id)
            .ok_or_else(|| Error::InvalidData(format!("account contract not found: {origin}")))?;
        let artifact = self
            .contract_store
            .get_artifact(&class_id)
            .await?
            .ok_or_else(|| {
                Error::InvalidData(format!(
                    "account contract artifact not found for class {class_id}"
                ))
            })?;

        // Find the entrypoint function — look up from the tx request's selector
        let entrypoint_args = request
            .args_of_calls
            .iter()
            .find(|hv| hv.hash == request.first_call_args_hash)
            .ok_or_else(|| {
                Error::InvalidData("firstCallArgsHash not found in argsOfCalls".into())
            })?;

        let function_name = {
            let selector_field = tx_request
                .data
                .get("functionSelector")
                .and_then(|v| v.as_str())
                .and_then(|s| aztec_core::abi::FunctionSelector::from_hex(s).ok());
            if let Some(sel) = selector_field {
                self.resolve_function_name_by_selector(&origin, sel).await
            } else {
                // Default to "entrypoint" for account contracts
                "entrypoint".to_owned()
            }
        };

        let function = artifact.find_function(&function_name)?;

        // Build the ACVM witness for the entrypoint
        let full_witness = self.build_private_witness(
            &artifact,
            &function_name,
            &entrypoint_args.values,
            origin, // contract_address = account address
            origin, // msg_sender = self-call
            tx_request,
            anchor,
            function
                .selector
                .unwrap_or_else(aztec_core::abi::FunctionSelector::empty),
            function.is_static,
        );

        // Create the oracle with pre-seeded execution cache
        let mut oracle = crate::execution::PrivateExecutionOracle::new(
            &self.node,
            &self.contract_store,
            &self.key_store,
            &self.note_store,
            &self.capsule_store,
            &self.address_store,
            &self.sender_tagging_store,
            anchor.data.clone(),
            origin,
            protocol_nullifier,
            Some(origin),
            scopes.to_vec(),
            false,
        );

        // Pre-populate execution cache with all hashed values from the tx request
        // (mirrors TS: HashedValuesCache.create(request.argsOfCalls))
        oracle.seed_execution_cache(&request.args_of_calls);

        // Set auth witnesses from the tx request
        if let Some(auth_witnesses) = tx_request.data.get("authWitnesses").and_then(|v| {
            serde_json::from_value::<Vec<aztec_core::tx::AuthWitness>>(v.clone()).ok()
        }) {
            let pairs: Vec<(Fr, Vec<Fr>)> = auth_witnesses
                .iter()
                .map(|aw| (aw.request_hash, aw.fields.clone()))
                .collect();
            oracle.set_auth_witnesses(pairs);
        }

        // Store block-header + tx-context for nested calls
        let context_inputs_size = artifact.private_context_inputs_size(&function_name);
        if context_inputs_size > 5 {
            oracle.context_witness_prefix =
                full_witness[4..context_inputs_size.saturating_sub(1)].to_vec();
        }

        // Execute the entrypoint via ACVM
        let acvm_output = crate::execution::AcvmExecutor::execute_private(
            &artifact,
            &function_name,
            &full_witness,
            &mut oracle,
        )
        .await?;

        // Extract public call requests and calldata from oracle
        let public_call_requests = oracle.take_public_call_requests();
        let public_function_calldata = oracle.take_public_function_calldata();
        let teardown_request = oracle.take_teardown_call_request();

        // Build the execution result from oracle state
        let entrypoint_result = crate::execution::execution_result::PrivateCallExecutionResult {
            contract_address: origin,
            call_context: aztec_core::kernel_types::CallContext {
                msg_sender: origin,
                contract_address: origin,
                function_selector: function
                    .selector
                    .map(|s| s.to_field())
                    .unwrap_or(Fr::zero()),
                is_static_call: false,
            },
            acir: acvm_output.acir_bytecode.clone(),
            vk: Vec::new(),
            partial_witness: acvm_output.witness.clone(),
            return_values: acvm_output.return_values.clone(),
            new_notes: oracle.new_notes.clone(),
            note_hash_nullifier_counter_map: oracle.note_hash_nullifier_counter_map.clone(),
            offchain_effects: Vec::new(),
            pre_tags: Vec::new(),
            note_hashes: oracle.note_hashes.clone(),
            nullifiers: oracle.nullifiers.clone(),
            private_logs: oracle.private_logs.clone(),
            contract_class_logs: oracle.contract_class_logs.clone(),
            public_call_requests,
            public_teardown_call_request: teardown_request,
            start_side_effect_counter: 2,
            end_side_effect_counter: oracle.side_effect_counter,
            min_revertible_side_effect_counter: oracle.min_revertible_side_effect_counter,
            note_hash_read_requests: oracle.note_hash_read_requests.clone(),
            nullifier_read_requests: oracle.nullifier_read_requests.clone(),
            nested_execution_results: oracle.nested_results.clone(),
        };

        let contract_class_log_fields = entrypoint_result
            .contract_class_logs
            .iter()
            .map(|log| aztec_core::tx::ContractClassLogFields {
                fields: log.log.fields.clone(),
            })
            .collect();
        let expiration_timestamp = Self::extract_expiration_timestamp_from_witness(
            &acvm_output.witness,
            full_witness.len(),
            context_inputs_size,
        );

        Ok(CallExecutionBundle {
            simulated_return_values: if !acvm_output.first_acir_call_return_values.is_empty() {
                acvm_output.first_acir_call_return_values.clone()
            } else {
                acvm_output.return_values.clone()
            },
            first_acir_call_return_values: acvm_output.first_acir_call_return_values,
            execution_result: crate::execution::execution_result::PrivateExecutionResult {
                entrypoint: entrypoint_result,
                first_nullifier: protocol_nullifier,
                expiration_timestamp,
                public_function_calldata: public_function_calldata.clone(),
            },
            contract_class_log_fields,
            public_function_calldata,
        })
    }

    /// Public functions are executed by the sequencer, not the PXE. This method
    /// creates a minimal `PrivateExecutionResult` that enqueues the public call
    /// so the simulated kernel can package it correctly.
    fn build_public_call_execution(
        artifact: &aztec_core::abi::ContractArtifact,
        function_name: &str,
        encoded_args: &[Fr],
        contract_address: AztecAddress,
        origin: AztecAddress,
        first_nullifier: Fr,
        hide_msg_sender: bool,
        is_static_call: bool,
    ) -> Result<
        (
            crate::execution::execution_result::PrivateExecutionResult,
            Vec<aztec_core::tx::ContractClassLogFields>,
            Vec<aztec_core::tx::HashedValues>,
        ),
        Error,
    > {
        use crate::execution::execution_result::{
            PrivateCallExecutionResult, PrivateExecutionResult, PublicCallRequestData,
        };

        let args = encoded_args.to_vec();

        // Build calldata: function selector + args
        let func = artifact.find_function(function_name)?;
        let selector_fr: Fr = func
            .selector
            .ok_or_else(|| Error::InvalidData("public function has no selector".into()))?
            .into();
        let mut calldata = vec![selector_fr];
        calldata.extend_from_slice(&args);
        let hashed = aztec_core::tx::HashedValues::from_calldata(calldata);
        let calldata_hash = hashed.hash();
        let msg_sender = if hide_msg_sender {
            AztecAddress::zero()
        } else {
            origin
        };

        let entrypoint = PrivateCallExecutionResult {
            contract_address: origin,
            public_call_requests: vec![PublicCallRequestData {
                contract_address,
                msg_sender,
                is_static_call,
                calldata_hash,
                counter: 2,
            }],
            start_side_effect_counter: 2,
            min_revertible_side_effect_counter: 2,
            end_side_effect_counter: 3,
            ..Default::default()
        };

        let exec_result = PrivateExecutionResult {
            entrypoint,
            first_nullifier,
            expiration_timestamp: 0,
            public_function_calldata: vec![hashed.clone()],
        };

        Ok((exec_result, vec![], vec![hashed]))
    }

    async fn is_registered_account(&self, address: &AztecAddress) -> Result<bool, Error> {
        let Some(complete) = self.address_store.get(address).await? else {
            return Ok(false);
        };
        let accounts = self.key_store.get_accounts().await?;
        Ok(accounts.contains(&complete.public_keys.hash()))
    }
}

fn public_function_signatures(artifact: &ContractArtifact) -> Vec<String> {
    artifact
        .functions
        .iter()
        .filter(|function| function.function_type == FunctionType::Public)
        .map(|function| {
            let params = function
                .parameters
                .iter()
                .map(|param| abi_type_signature(&param.typ))
                .collect::<Vec<_>>()
                .join(",");
            format!("{}({params})", function.name)
        })
        .collect()
}

#[async_trait]
impl<N: AztecNode + Clone + 'static> Pxe for EmbeddedPxe<N> {
    async fn get_synced_block_header(&self) -> Result<BlockHeader, Error> {
        let anchor = self.get_anchor_block_header().await?;
        Ok(BlockHeader { data: anchor.data })
    }

    async fn get_contract_instance(
        &self,
        address: &AztecAddress,
    ) -> Result<Option<ContractInstanceWithAddress>, Error> {
        // Check local store first
        if let Some(inst) = self.contract_store.get_instance(address).await? {
            return Ok(Some(inst));
        }
        // Fall through to node
        self.node.get_contract(address).await
    }

    async fn get_contract_artifact(&self, id: &Fr) -> Result<Option<ContractArtifact>, Error> {
        self.contract_store.get_artifact(id).await
    }

    async fn get_contracts(&self) -> Result<Vec<AztecAddress>, Error> {
        self.contract_store.get_contract_addresses().await
    }

    async fn register_account(
        &self,
        secret_key: &Fr,
        partial_address: &Fr,
    ) -> Result<CompleteAddress, Error> {
        tracing::debug!("registering account");

        // Derive keys and store in key store
        let _derived = self.key_store.add_account(secret_key).await?;

        // Derive complete address
        let complete =
            complete_address_from_secret_key_and_partial_address(secret_key, partial_address)?;

        // Store in address store
        self.address_store.add(&complete).await?;

        tracing::debug!(address = %complete.address, "account registered");
        Ok(complete)
    }

    async fn get_registered_accounts(&self) -> Result<Vec<CompleteAddress>, Error> {
        let accounts = self.key_store.get_accounts().await?;
        let complete_addresses = self.address_store.get_all().await?;
        Ok(complete_addresses
            .into_iter()
            .filter(|complete| accounts.contains(&complete.public_keys.hash()))
            .collect())
    }

    async fn register_sender(&self, sender: &AztecAddress) -> Result<AztecAddress, Error> {
        if self.is_registered_account(sender).await? {
            return Ok(*sender);
        }
        self.sender_store.add(sender).await?;
        Ok(*sender)
    }

    async fn get_senders(&self) -> Result<Vec<AztecAddress>, Error> {
        self.sender_store.get_all().await
    }

    async fn remove_sender(&self, sender: &AztecAddress) -> Result<(), Error> {
        self.sender_store.remove(sender).await
    }

    async fn register_contract_class(&self, artifact: &ContractArtifact) -> Result<(), Error> {
        tracing::debug!(name = %artifact.name, "registering contract class");
        self.contract_store.add_class(artifact).await?;
        Ok(())
    }

    async fn register_contract(&self, request: RegisterContractRequest) -> Result<(), Error> {
        tracing::debug!(address = %request.instance.address, "registering contract");

        if let Some(ref artifact) = request.artifact {
            let computed_class_id = compute_contract_class_id_from_artifact(artifact)?;
            if computed_class_id != request.instance.inner.current_contract_class_id {
                return Err(Error::InvalidData(format!(
                    "artifact class id {} does not match instance class id {}",
                    computed_class_id, request.instance.inner.current_contract_class_id
                )));
            }

            let computed_address = compute_contract_address_from_instance(&request.instance.inner)?;
            if computed_address != request.instance.address {
                return Err(Error::InvalidData(format!(
                    "artifact instance address {} does not match computed contract address {}",
                    request.instance.address, computed_address
                )));
            }

            let public_function_signatures = public_function_signatures(artifact);
            if !public_function_signatures.is_empty() {
                self.node
                    .register_contract_function_signatures(&public_function_signatures)
                    .await?;
            }
        } else if self
            .contract_store
            .get_artifact(&request.instance.inner.current_contract_class_id)
            .await?
            .is_none()
        {
            return Err(Error::InvalidData(format!(
                "artifact not found for contract class {}",
                request.instance.inner.current_contract_class_id
            )));
        }

        // Store the instance
        self.contract_store.add_instance(&request.instance).await?;

        // Store the artifact if provided
        if let Some(ref artifact) = request.artifact {
            self.contract_store
                .add_artifact(&request.instance.inner.current_contract_class_id, artifact)
                .await?;
        }

        Ok(())
    }

    async fn update_contract(
        &self,
        address: &AztecAddress,
        artifact: &ContractArtifact,
    ) -> Result<(), Error> {
        self.contract_store.update_artifact(address, artifact).await
    }

    async fn simulate_tx(
        &self,
        tx_request: &TxExecutionRequest,
        opts: SimulateTxOpts,
    ) -> Result<TxSimulationResult, Error> {
        self.sync_block_state().await?;

        let anchor = self.get_anchor_block_header().await?;
        let protocol_nullifier = compute_protocol_nullifier(&Self::tx_request_hash(tx_request)?);
        let request: DecodedTxExecutionRequest = serde_json::from_value(tx_request.data.clone())?;
        let origin = request.origin;
        let decoded_calls = Self::decode_entrypoint_calls(&request)?;
        let mut bundles = Vec::with_capacity(decoded_calls.len());
        for call in &decoded_calls {
            bundles.push(
                self.execute_entrypoint_call_bundle(
                    tx_request,
                    call,
                    origin,
                    protocol_nullifier,
                    &anchor,
                    &opts.scopes,
                )
                .await?,
            );
        }
        let bundled_private_return_values: Vec<Vec<String>> = bundles
            .iter()
            .map(|bundle| {
                bundle
                    .simulated_return_values
                    .iter()
                    .map(|f| f.to_string())
                    .collect()
            })
            .collect();
        let aggregated = Self::aggregate_call_bundles(origin, bundles);

        // Verify auth witness signatures.
        //
        // The oracle-based private execution does not evaluate Noir circuit
        // constraints, so Schnorr/ECDSA signature checks inside the account
        // contract entrypoint are skipped.  We replicate that check here so
        // that `simulate_tx` rejects transactions signed with an invalid key,
        // matching the behavior of the upstream TS ACIR simulator.
        self.verify_auth_witness_signatures(tx_request, origin)
            .await?;

        // Process through simulated kernel
        let kernel_output = crate::kernel::SimulatedKernel::process(
            &aggregated.execution_result,
            aztec_core::kernel_types::TxConstantData::default(), // TODO: fill from block header
            &origin.0,
            0, // TODO: fill expiration timestamp
        )?;

        let pi = &kernel_output.public_inputs;

        // Extract private return values — mirrors TS `getPrivateReturnValues()`.
        //
        // The aggregated execution tree has three levels:
        //   1. Synthetic root (aggregated entrypoint)
        //   2. Account contract entrypoint (first nested_execution_results)
        //   3. User's function call (first nested of the account entrypoint)
        //
        // TS equivalent: `simulatedTx.getPrivateReturnValues().nested[0].values`
        // which is `entrypoint.nestedExecutionResults[0].returnValues`.
        //
        // In the Rust aggregated tree the account entrypoint sits at level [0],
        // so the user's call is at [0][0].
        // Extract private return values — mirrors TS `getPrivateReturnValues()`.
        //
        // For private functions with databus returns, the main circuit's ACIR
        // return witnesses are the full PCPI — the user's actual return values
        // come from the first inner ACIR sub-circuit call, stored in
        // `first_acir_call_return_values`.
        //
        // Walk the execution tree first; fall back to the bundle-level ACIR
        // call return values if the tree has nothing.
        let private_return_values: Vec<String> = {
            let from_tree: Vec<String> = aggregated
                .execution_result
                .entrypoint
                .nested_execution_results
                .first()
                .and_then(|ep| {
                    // If the entrypoint has nested calls (account contract path),
                    // the user's function is the first nested call.
                    if !ep.nested_execution_results.is_empty() {
                        ep.nested_execution_results.first()
                    } else {
                        // Direct execution: the entrypoint IS the user's function.
                        Some(ep)
                    }
                })
                .map(|r| r.return_values.iter().map(|f| f.to_string()).collect())
                .unwrap_or_default();

            if from_tree.is_empty() && !aggregated.first_acir_call_return_values.is_empty() {
                // Databus return path: use first ACIR sub-circuit return values.
                aggregated
                    .first_acir_call_return_values
                    .iter()
                    .map(|f| f.to_string())
                    .collect()
            } else {
                from_tree
            }
        };

        let return_values = if bundled_private_return_values.len() > 1 {
            serde_json::Value::Array(
                bundled_private_return_values
                    .into_iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(Error::from)?,
            )
        } else {
            serde_json::json!({
                "returnValues": private_return_values,
            })
        };

        Ok(TxSimulationResult {
            data: serde_json::json!({
                "returnValues": return_values,
                "gasUsed": {
                    "daGas": pi.gas_used.da_gas,
                    "l2Gas": pi.gas_used.l2_gas,
                },
                "isForPublic": pi.is_for_public(),
            }),
        })
    }

    async fn prove_tx(
        &self,
        tx_request: &TxExecutionRequest,
        scopes: Vec<AztecAddress>,
    ) -> Result<TxProvingResult, Error> {
        self.sync_block_state().await?;

        let anchor = self.get_anchor_block_header().await?;
        let tx_req_hash = Self::tx_request_hash(tx_request)?;
        let protocol_nullifier = compute_protocol_nullifier(&tx_req_hash);
        let origin = self.extract_origin(tx_request);
        let mut tx_constants = Self::build_tx_constant_data(
            &anchor,
            tx_request,
            self.vk_tree_root,
            self.protocol_contracts_hash,
        );
        Self::ensure_min_fees(&mut tx_constants, &anchor);
        let request: DecodedTxExecutionRequest = serde_json::from_value(tx_request.data.clone())?;
        let fee_payer = request.fee_payer.unwrap_or(origin);
        let decoded_calls = Self::decode_entrypoint_calls(&request)?;

        for call in &decoded_calls {
            for _scope in &scopes {
                let _ = self
                    .contract_sync_service
                    .ensure_contract_synced(
                        &call.to,
                        &scopes,
                        &anchor.block_hash,
                        |contract, scopes| async move {
                            self.execute_sync_state_for_contract(contract, scopes).await
                        },
                    )
                    .await;
            }
        }

        // If any call is public, run the account entrypoint through ACVM
        // (like the TS SDK). The Noir entrypoint handles public calls via
        // `enqueuePublicFunctionCall` which the oracle captures correctly.
        // The decode-and-dispatch path only works for private-only payloads.
        //
        // We detect public calls from the parsed entrypoint payload (the
        // `is_public` flag in the encoded call fields).
        let has_public_calls = {
            let entrypoint_args = request
                .args_of_calls
                .iter()
                .find(|hv| hv.hash == request.first_call_args_hash);
            if let Some(ea) = entrypoint_args {
                parse_encoded_calls(&ea.values)
                    .map(|calls| calls.iter().any(|c| c.is_public))
                    .unwrap_or(false)
            } else {
                false
            }
        };

        let aggregated = if has_public_calls {
            match self
                .execute_entrypoint_via_acvm(
                    tx_request,
                    origin,
                    protocol_nullifier,
                    &anchor,
                    &scopes,
                )
                .await
            {
                Ok(bundle) => bundle,
                Err(Error::InvalidData(msg))
                    if msg.contains("account contract not found")
                        || msg.contains("account contract artifact not found") =>
                {
                    let mut bundles = Vec::with_capacity(decoded_calls.len());
                    for call in &decoded_calls {
                        bundles.push(
                            self.execute_entrypoint_call_bundle(
                                tx_request,
                                call,
                                origin,
                                protocol_nullifier,
                                &anchor,
                                &scopes,
                            )
                            .await?,
                        );
                    }
                    Self::aggregate_call_bundles(origin, bundles)
                }
                Err(err) => return Err(err),
            }
        } else {
            let mut bundles = Vec::with_capacity(decoded_calls.len());
            for call in &decoded_calls {
                bundles.push(
                    self.execute_entrypoint_call_bundle(
                        tx_request,
                        call,
                        origin,
                        protocol_nullifier,
                        &anchor,
                        &scopes,
                    )
                    .await?,
                );
            }
            Self::aggregate_call_bundles(origin, bundles)
        };
        let expiration_timestamp = if aggregated.execution_result.expiration_timestamp != 0 {
            aggregated.execution_result.expiration_timestamp
        } else {
            Self::compute_expiration(&anchor)
        };

        // Verify auth witness signatures (same check as simulate_tx).
        self.verify_auth_witness_signatures(tx_request, origin)
            .await?;

        // Sort public_function_calldata to match the counter-sorted order
        // of public call requests (the simulated kernel sorts requests by
        // counter, so the calldata must follow the same order).
        let sorted_calldata = {
            let requests = aggregated.execution_result.all_public_call_requests();
            let calldata = &aggregated.public_function_calldata;
            if requests.len() == calldata.len() && !calldata.is_empty() {
                let mut paired: Vec<_> =
                    requests.into_iter().zip(calldata.iter().cloned()).collect();
                paired.sort_by_key(|(req, _)| req.counter);
                paired.into_iter().map(|(_, cd)| cd).collect()
            } else {
                aggregated.public_function_calldata.clone()
            }
        };

        // Process through simulated kernel
        let kernel_output = crate::kernel::SimulatedKernel::process(
            &aggregated.execution_result,
            tx_constants,
            &fee_payer.0,
            expiration_timestamp,
        )?;
        // Serialize kernel output to buffer and build TxProvingResult
        let tx_hash = aztec_core::tx::TxHash(kernel_output.public_inputs.hash().to_be_bytes());
        let public_inputs_buffer = kernel_output.public_inputs.to_buffer();
        let public_inputs =
            aztec_core::tx::PrivateKernelTailCircuitPublicInputs::from_bytes(public_inputs_buffer);
        // ChonkProof TS format: 4-byte BE field count + N×32-byte Fr fields
        let mut chonk_bytes =
            Vec::with_capacity(4 + aztec_core::constants::CHONK_PROOF_LENGTH * 32);
        chonk_bytes
            .extend_from_slice(&(aztec_core::constants::CHONK_PROOF_LENGTH as u32).to_be_bytes());
        chonk_bytes.resize(4 + aztec_core::constants::CHONK_PROOF_LENGTH * 32, 0);
        let chonk_proof = aztec_core::tx::ChonkProof::from_bytes(chonk_bytes);

        Ok(TxProvingResult {
            tx_hash: Some(tx_hash),
            private_execution_result: serde_json::json!({}),
            public_inputs,
            chonk_proof,
            contract_class_log_fields: aggregated.contract_class_log_fields,
            public_function_calldata: sorted_calldata,
            stats: None,
        })
    }

    async fn profile_tx(
        &self,
        tx_request: &TxExecutionRequest,
        opts: ProfileTxOpts,
    ) -> Result<TxProfileResult, Error> {
        self.sync_block_state().await?;

        let anchor = self.get_anchor_block_header().await?;
        let protocol_nullifier = compute_protocol_nullifier(&Self::tx_request_hash(tx_request)?);
        let request: DecodedTxExecutionRequest = serde_json::from_value(tx_request.data.clone())?;
        let origin = request.origin;
        let decoded_calls = Self::decode_entrypoint_calls(&request)?;
        let fee_payer = request.fee_payer.unwrap_or(origin);
        let mut bundles = Vec::with_capacity(decoded_calls.len());
        for call in &decoded_calls {
            bundles.push(
                self.execute_entrypoint_call_bundle(
                    tx_request,
                    call,
                    origin,
                    protocol_nullifier,
                    &anchor,
                    &opts.scopes,
                )
                .await?,
            );
        }
        let aggregated = Self::aggregate_call_bundles(origin, bundles);

        let mut tx_constants = Self::build_tx_constant_data(
            &anchor,
            tx_request,
            self.vk_tree_root,
            self.protocol_contracts_hash,
        );
        let expiration_timestamp = if aggregated.execution_result.expiration_timestamp != 0 {
            aggregated.execution_result.expiration_timestamp
        } else {
            Self::compute_expiration(&anchor)
        };
        Self::ensure_min_fees(&mut tx_constants, &anchor);

        let _kernel_output = crate::kernel::SimulatedKernel::process(
            &aggregated.execution_result,
            tx_constants,
            &fee_payer.0,
            expiration_timestamp,
        )?;

        let data = serde_json::json!({
            "expirationTimestamp": expiration_timestamp,
            "data": {
                "expirationTimestamp": expiration_timestamp,
            },
        });

        Ok(TxProfileResult { data })
    }

    async fn execute_utility(
        &self,
        call: &FunctionCall,
        opts: ExecuteUtilityOpts,
    ) -> Result<UtilityExecutionResult, Error> {
        self.sync_block_state().await?;
        // Always wipe the contract sync cache before utility execution.
        // This ensures we pick up nullifier-tree changes from recently
        // mined transactions (which may share the same block number).
        self.contract_sync_service.wipe().await;
        let anchor = self.get_anchor_block_header().await?;

        // Look up the artifact for the target contract
        let contract_instance = self.contract_store.get_instance(&call.to).await?;
        let class_id = contract_instance
            .as_ref()
            .map(|i| i.inner.current_contract_class_id)
            .ok_or_else(|| Error::InvalidData(format!("contract not found: {}", call.to)))?;

        let artifact = self
            .contract_store
            .get_artifact(&class_id)
            .await?
            .ok_or_else(|| {
                Error::InvalidData(format!("artifact not found for class {class_id}"))
            })?;

        // Find the function by selector
        let function = artifact
            .find_function_by_selector(&call.selector)
            .or_else(|| {
                // Fallback: try finding by name from selector string
                artifact
                    .functions
                    .iter()
                    .find(|f| f.function_type == FunctionType::Utility)
            })
            .ok_or_else(|| {
                Error::InvalidData(format!(
                    "utility function with selector {} not found in {}",
                    call.selector, artifact.name
                ))
            })?;

        let function_name = function.name.clone();

        if function_name != "sync_state" {
            self.contract_sync_service
                .ensure_contract_synced(
                    &call.to,
                    &opts.scopes,
                    &anchor.block_hash,
                    |contract, scopes| async move {
                        self.execute_sync_state_for_contract(contract, scopes).await
                    },
                )
                .await?;
        }

        // Encode arguments as field elements (recursively flatten structs/arrays)
        let args: Vec<Fr> = call
            .args
            .iter()
            .flat_map(Self::abi_value_to_fields)
            .collect();

        // Create utility oracle
        let mut oracle = crate::execution::UtilityExecutionOracle::new(
            &self.node,
            &self.contract_store,
            &self.key_store,
            &self.note_store,
            &self.address_store,
            &self.capsule_store,
            &self.sender_store,
            &self.sender_tagging_store,
            &self.recipient_tagging_store,
            &self.private_event_store,
            &self.anchor_block_store,
            anchor.data.clone(),
            call.to,
            opts.scopes.clone(),
        );

        // Set auth witnesses if provided
        let auth_witness_pairs: Vec<(Fr, Vec<Fr>)> = opts
            .authwits
            .iter()
            .map(|aw| (aw.request_hash, aw.fields.clone()))
            .collect();
        oracle.set_auth_witnesses(auth_witness_pairs);

        // Execute via ACVM
        let result = crate::execution::AcvmExecutor::execute_utility(
            &artifact,
            &function_name,
            &args,
            &mut oracle,
        )
        .await?;

        Ok(UtilityExecutionResult {
            result: result.return_values,
            stats: None,
        })
    }

    async fn get_private_events(
        &self,
        event_selector: &EventSelector,
        filter: PrivateEventFilter,
    ) -> Result<Vec<PackedPrivateEvent>, Error> {
        // Phase 3: Private event retrieval
        //
        // Matching the TS PXE.getPrivateEvents flow:
        // 1. Sync block state
        // 2. Get anchor block number
        // 3. Ensure contract synced (when ACVM is available)
        // 4. Validate filter
        // 5. Query PrivateEventStore
        // 6. Convert to PackedPrivateEvent

        self.sync_block_state().await?;

        let anchor_block_number = self.get_anchor_block_number().await?;

        // Ensure contract is synced for the filter's contract address
        // (when ACVM is available, this runs the contract's sync_state function)
        let anchor_hash = self
            .get_anchor_block_header()
            .await
            .map(|h| h.block_hash)
            .unwrap_or_default();
        self.contract_sync_service
            .ensure_contract_synced(
                &filter.contract_address,
                &filter.scopes,
                &anchor_hash,
                |contract, scopes| async move {
                    self.execute_sync_state_for_contract(contract, scopes).await
                },
            )
            .await?;

        // Validate and sanitize the filter
        let validator = PrivateEventFilterValidator::new(anchor_block_number);
        let query_filter = validator.validate(&filter)?;

        tracing::debug!(
            contract = %filter.contract_address,
            from_block = ?query_filter.from_block,
            to_block = ?query_filter.to_block,
            "getting private events"
        );

        // Query the store
        let stored_events = self
            .private_event_store
            .get_private_events(event_selector, &query_filter)
            .await?;

        // Convert StoredPrivateEvent → PackedPrivateEvent
        let packed_events: Vec<PackedPrivateEvent> = stored_events
            .into_iter()
            .map(|e| {
                let l2_block_hash =
                    aztec_pxe_client::BlockHash::from_hex(&e.l2_block_hash).unwrap_or_default();

                PackedPrivateEvent {
                    packed_event: e.msg_content,
                    tx_hash: e.tx_hash,
                    l2_block_number: e.l2_block_number,
                    l2_block_hash,
                    event_selector: e.event_selector,
                }
            })
            .collect();

        Ok(packed_events)
    }

    async fn stop(&self) -> Result<(), Error> {
        tracing::debug!("EmbeddedPxe stopped");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aztec_core::types::{ContractInstance, PublicKeys};
    use std::sync::Mutex;

    fn sample_public_artifact() -> ContractArtifact {
        ContractArtifact::from_json(
            r#"{
                "name": "Counter",
                "functions": [
                    {
                        "name": "constructor",
                        "function_type": "private",
                        "is_initializer": true,
                        "is_static": false,
                        "parameters": [],
                        "return_types": [],
                        "selector": "0xe5fb6c81",
                        "bytecode": "0x01"
                    },
                    {
                        "name": "increment",
                        "function_type": "public",
                        "is_initializer": false,
                        "is_static": false,
                        "parameters": [
                            { "name": "value", "type": { "kind": "field" } }
                        ],
                        "return_types": [],
                        "selector": "0x12345678",
                        "bytecode": "0x01"
                    }
                ]
            }"#,
        )
        .unwrap()
    }

    /// A minimal mock AztecNode for testing.
    #[derive(Clone)]
    struct MockNode {
        registered_signatures: Arc<Mutex<Vec<String>>>,
    }

    impl Default for MockNode {
        fn default() -> Self {
            Self {
                registered_signatures: Arc::new(Mutex::new(vec![])),
            }
        }
    }

    #[async_trait]
    impl AztecNode for MockNode {
        async fn get_node_info(&self) -> Result<aztec_node_client::NodeInfo, Error> {
            Ok(aztec_node_client::NodeInfo {
                node_version: "mock".into(),
                l1_chain_id: 1,
                rollup_version: 1,
                enr: None,
                l1_contract_addresses: serde_json::Value::Null,
                protocol_contract_addresses: serde_json::Value::Null,
                real_proofs: false,
                l2_circuits_vk_tree_root: None,
                l2_protocol_contracts_hash: None,
            })
        }
        async fn get_block_number(&self) -> Result<u64, Error> {
            Ok(1)
        }
        async fn get_proven_block_number(&self) -> Result<u64, Error> {
            Ok(1)
        }
        async fn get_tx_receipt(
            &self,
            _tx_hash: &aztec_core::tx::TxHash,
        ) -> Result<aztec_core::tx::TxReceipt, Error> {
            Err(Error::InvalidData("mock".into()))
        }
        async fn get_tx_effect(
            &self,
            _tx_hash: &aztec_core::tx::TxHash,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_tx_by_hash(
            &self,
            _tx_hash: &aztec_core::tx::TxHash,
        ) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_public_logs(
            &self,
            _filter: aztec_node_client::PublicLogFilter,
        ) -> Result<aztec_node_client::PublicLogsResponse, Error> {
            Ok(aztec_node_client::PublicLogsResponse {
                logs: vec![],
                max_logs_hit: false,
            })
        }
        async fn send_tx(&self, _tx: &serde_json::Value) -> Result<(), Error> {
            Err(Error::InvalidData("mock".into()))
        }
        async fn get_contract(
            &self,
            _address: &AztecAddress,
        ) -> Result<Option<ContractInstanceWithAddress>, Error> {
            Ok(None)
        }
        async fn get_contract_class(&self, _id: &Fr) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
        async fn get_block_header(&self, _block_number: u64) -> Result<serde_json::Value, Error> {
            Ok(serde_json::json!({"globalVariables": {"blockNumber": 1}, "blockHash": "0x01"}))
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
        ) -> Result<aztec_node_client::TxValidationResult, Error> {
            Ok(aztec_node_client::TxValidationResult::Valid)
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
            signatures: &[String],
        ) -> Result<(), Error> {
            self.registered_signatures
                .lock()
                .unwrap()
                .extend(signatures.iter().cloned());
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

    #[tokio::test]
    async fn create_and_register_account() {
        let pxe = EmbeddedPxe::create_ephemeral(MockNode::default())
            .await
            .unwrap();
        let sk = Fr::from(8923u64);
        let partial = Fr::from(243523u64);
        let complete = pxe.register_account(&sk, &partial).await.unwrap();
        assert_eq!(complete.partial_address, partial);

        let accounts = pxe.get_registered_accounts().await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].address, complete.address);
    }

    #[tokio::test]
    async fn register_and_retrieve_contract() {
        use aztec_pxe_client::RegisterContractRequest;

        let pxe = EmbeddedPxe::create_ephemeral(MockNode::default())
            .await
            .unwrap();
        let artifact = sample_public_artifact();
        let class_id = compute_contract_class_id_from_artifact(&artifact).unwrap();
        let inner = ContractInstance {
            version: 1,
            salt: Fr::from(1u64),
            deployer: AztecAddress::zero(),
            current_contract_class_id: class_id,
            original_contract_class_id: class_id,
            initialization_hash: Fr::zero(),
            public_keys: PublicKeys::default(),
        };
        let address = compute_contract_address_from_instance(&inner).unwrap();
        let instance = ContractInstanceWithAddress { address, inner };

        pxe.register_contract_class(&artifact).await.unwrap();

        pxe.register_contract(RegisterContractRequest {
            instance: instance.clone(),
            artifact: None,
        })
        .await
        .unwrap();

        let retrieved = pxe.get_contract_instance(&instance.address).await.unwrap();
        assert!(retrieved.is_some());

        let contracts = pxe.get_contracts().await.unwrap();
        assert_eq!(contracts.len(), 1);
    }

    #[tokio::test]
    async fn sender_management() {
        let pxe = EmbeddedPxe::create_ephemeral(MockNode::default())
            .await
            .unwrap();
        let sender = AztecAddress::from(99u64);

        pxe.register_sender(&sender).await.unwrap();
        let senders = pxe.get_senders().await.unwrap();
        assert_eq!(senders.len(), 1);

        pxe.remove_sender(&sender).await.unwrap();
        let senders = pxe.get_senders().await.unwrap();
        assert!(senders.is_empty());
    }

    #[tokio::test]
    async fn block_header_sync() {
        let pxe = EmbeddedPxe::create_ephemeral(MockNode::default())
            .await
            .unwrap();
        let header = pxe.get_synced_block_header().await.unwrap();
        assert!(header.data.is_object());
    }

    #[tokio::test]
    async fn register_sender_ignores_registered_accounts() {
        let pxe = EmbeddedPxe::create_ephemeral(MockNode::default())
            .await
            .unwrap();
        let sk = Fr::from(77u64);
        let partial = Fr::from(123u64);
        let complete = pxe.register_account(&sk, &partial).await.unwrap();

        pxe.register_sender(&complete.address).await.unwrap();

        assert!(pxe.get_senders().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn register_contract_validates_artifact_and_registers_public_signatures() {
        use aztec_pxe_client::RegisterContractRequest;

        let node = MockNode::default();
        let artifact = sample_public_artifact();
        let class_id = compute_contract_class_id_from_artifact(&artifact).unwrap();
        let inner = ContractInstance {
            version: 1,
            salt: Fr::from(5u64),
            deployer: AztecAddress::zero(),
            current_contract_class_id: class_id,
            original_contract_class_id: class_id,
            initialization_hash: Fr::zero(),
            public_keys: PublicKeys::default(),
        };
        let address = compute_contract_address_from_instance(&inner).unwrap();
        let instance = ContractInstanceWithAddress { address, inner };

        let pxe = EmbeddedPxe::create_ephemeral(node).await.unwrap();
        pxe.register_contract(RegisterContractRequest {
            instance,
            artifact: Some(artifact),
        })
        .await
        .unwrap();

        let registered = pxe.node().registered_signatures.lock().unwrap().clone();
        assert_eq!(registered, vec!["increment(Field)".to_owned()]);
    }

    #[tokio::test]
    async fn register_contract_rejects_missing_artifact_for_unknown_class() {
        use aztec_pxe_client::RegisterContractRequest;

        let pxe = EmbeddedPxe::create_ephemeral(MockNode::default())
            .await
            .unwrap();
        let instance = ContractInstanceWithAddress {
            address: AztecAddress::from(42u64),
            inner: ContractInstance {
                version: 1,
                salt: Fr::from(1u64),
                deployer: AztecAddress::zero(),
                current_contract_class_id: Fr::from(999u64),
                original_contract_class_id: Fr::from(999u64),
                initialization_hash: Fr::zero(),
                public_keys: PublicKeys::default(),
            },
        };

        let err = pxe
            .register_contract(RegisterContractRequest {
                instance,
                artifact: None,
            })
            .await
            .unwrap_err();

        assert!(err.to_string().contains("artifact not found"));
    }

    #[tokio::test]
    async fn get_private_events_returns_empty_when_no_events() {
        let pxe = EmbeddedPxe::create_ephemeral(MockNode::default())
            .await
            .unwrap();
        let events = pxe
            .get_private_events(
                &EventSelector(Fr::from(1u64)),
                PrivateEventFilter {
                    contract_address: AztecAddress::from(1u64),
                    scopes: vec![AztecAddress::from(99u64)],
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn get_private_events_rejects_empty_scopes() {
        let pxe = EmbeddedPxe::create_ephemeral(MockNode::default())
            .await
            .unwrap();
        let result = pxe
            .get_private_events(
                &EventSelector(Fr::from(1u64)),
                PrivateEventFilter {
                    contract_address: AztecAddress::from(1u64),
                    scopes: vec![],
                    ..Default::default()
                },
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn anchor_block_store_is_populated_after_create() {
        let pxe = EmbeddedPxe::create_ephemeral(MockNode::default())
            .await
            .unwrap();
        let anchor = pxe.anchor_block_store().get_block_header().await.unwrap();
        assert!(anchor.is_some());
    }
}
