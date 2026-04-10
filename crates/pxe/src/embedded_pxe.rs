//! Embedded PXE implementation that runs PXE logic in-process.

use std::sync::Arc;

use async_trait::async_trait;
use aztec_core::abi::{abi_type_signature, ContractArtifact, EventSelector, FunctionType};
use aztec_core::error::Error;
use aztec_core::hash::{
    compute_contract_address_from_instance, compute_contract_class_id_from_artifact,
};
use aztec_core::tx::FunctionCall;
use aztec_core::types::{AztecAddress, CompleteAddress, ContractInstanceWithAddress, Fr};
use aztec_crypto::complete_address_from_secret_key_and_partial_address;
use aztec_node_client::AztecNode;
use aztec_pxe_client::{
    BlockHeader, ExecuteUtilityOpts, PackedPrivateEvent, PrivateEventFilter, ProfileTxOpts, Pxe,
    RegisterContractRequest, SimulateTxOpts, TxExecutionRequest, TxProfileResult, TxProvingResult,
    TxSimulationResult, UtilityExecutionResult,
};
use tokio::sync::RwLock;

use crate::kernel::prover::{BbPrivateKernelProver, BbProverConfig};
use crate::stores::kv::KvStore;
use crate::stores::{
    AddressStore, CapsuleStore, ContractStore, KeyStore, NoteStore, PrivateEventStore,
    RecipientTaggingStore, SenderStore, SenderTaggingStore,
};
use crate::sync::BlockSynchronizer;

/// Embedded PXE that runs private execution logic in-process.
///
/// In-process PXE for Aztec v4.x where PXE runs client-side.
/// Talks to the Aztec node via `node_*` RPC methods and maintains local
/// stores for contracts, keys, addresses, notes, and capsules.
pub struct EmbeddedPxe<N: AztecNode> {
    node: N,
    contract_store: ContractStore,
    key_store: KeyStore,
    address_store: AddressStore,
    note_store: NoteStore,
    #[allow(dead_code)] // Used when ACVM integration is complete
    capsule_store: CapsuleStore,
    block_header: RwLock<Option<serde_json::Value>>,
    /// Registered sender addresses for private log discovery.
    sender_store: SenderStore,
    /// Phase 2: Sender tagging store for outgoing tag index tracking.
    #[allow(dead_code)] // Used when full prove_tx flow is wired
    sender_tagging_store: SenderTaggingStore,
    /// Phase 2: Recipient tagging store for incoming tag index tracking.
    #[allow(dead_code)] // Used when full prove_tx flow is wired
    recipient_tagging_store: RecipientTaggingStore,
    /// Phase 2: Private event store for discovered private events.
    #[allow(dead_code)] // Used when get_private_events is wired
    private_event_store: PrivateEventStore,
    /// Phase 2: Kernel prover for generating proofs via bb binary.
    kernel_prover: BbPrivateKernelProver,
}

impl<N: AztecNode> EmbeddedPxe<N> {
    /// Create a new EmbeddedPxe backed by the given node client and KV store.
    pub async fn create(node: N, kv: Arc<dyn KvStore>) -> Result<Self, Error> {
        Self::create_with_prover_config(node, kv, BbProverConfig::default()).await
    }

    /// Create a new EmbeddedPxe with custom BB prover configuration.
    pub async fn create_with_prover_config(
        node: N,
        kv: Arc<dyn KvStore>,
        prover_config: BbProverConfig,
    ) -> Result<Self, Error> {
        let contract_store = ContractStore::new(Arc::clone(&kv));
        let key_store = KeyStore::new(Arc::clone(&kv));
        let address_store = AddressStore::new(Arc::clone(&kv));
        let note_store = NoteStore::new(Arc::clone(&kv));
        let capsule_store = CapsuleStore::new(Arc::clone(&kv));
        let sender_store = SenderStore::new(Arc::clone(&kv));
        let sender_tagging_store = SenderTaggingStore::new(Arc::clone(&kv));
        let recipient_tagging_store = RecipientTaggingStore::new(Arc::clone(&kv));
        let private_event_store = PrivateEventStore::new(Arc::clone(&kv));
        let kernel_prover = BbPrivateKernelProver::new(prover_config);

        let pxe = Self {
            node,
            contract_store,
            key_store,
            address_store,
            note_store,
            capsule_store,
            block_header: RwLock::new(None),
            sender_store,
            sender_tagging_store,
            recipient_tagging_store,
            private_event_store,
            kernel_prover,
        };

        // Sync initial block header
        pxe.sync_block_header().await?;

        Ok(pxe)
    }

    /// Create a new EmbeddedPxe with an in-memory KV store.
    pub async fn create_ephemeral(node: N) -> Result<Self, Error> {
        let kv = Arc::new(crate::stores::InMemoryKvStore::new());
        Self::create(node, kv).await
    }

    /// Sync the block header from the node.
    async fn sync_block_header(&self) -> Result<serde_json::Value, Error> {
        let header = BlockSynchronizer::sync_block_header(&self.node).await?;
        let mut cached = self.block_header.write().await;
        *cached = Some(header.clone());
        Ok(header)
    }

    /// Get the current cached block header, syncing if necessary.
    async fn get_or_sync_block_header(&self) -> Result<serde_json::Value, Error> {
        let cached = self.block_header.read().await;
        if let Some(ref header) = *cached {
            return Ok(header.clone());
        }
        drop(cached);
        self.sync_block_header().await
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
impl<N: AztecNode + 'static> Pxe for EmbeddedPxe<N> {
    async fn get_synced_block_header(&self) -> Result<BlockHeader, Error> {
        let header = self.get_or_sync_block_header().await?;
        Ok(BlockHeader { data: header })
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
        _tx_request: &TxExecutionRequest,
        _opts: SimulateTxOpts,
    ) -> Result<TxSimulationResult, Error> {
        // Sync block header before simulation
        let _header = self.sync_block_header().await?;

        // TODO(Phase 1): Execute private functions via ACVM, then assemble
        // simulated kernel output. Requires acvm crate integration.
        //
        // The flow is:
        // 1. Parse TxExecutionRequest to extract function calls
        // 2. For each private call, execute via AcvmExecutor with PrivateExecutionOracle
        // 3. Run SimulatedKernel::process on the execution results
        // 4. Optionally simulate public calls via node.simulate_public_calls()
        // 5. Assemble TxSimulationResult
        Err(Error::InvalidData(
            "simulate_tx requires ACVM integration (pending Phase 1 completion)".into(),
        ))
    }

    async fn prove_tx(
        &self,
        _tx_request: &TxExecutionRequest,
        scopes: Vec<AztecAddress>,
    ) -> Result<TxProvingResult, Error> {
        // Phase 2: Full proving flow
        //
        // 1. Sync block header
        let header = self.sync_block_header().await?;

        // 2. Extract block hash from header for consistent state reads
        let block_hash = header
            .pointer("/blockHash")
            .and_then(|v| v.as_str())
            .and_then(|s| Fr::from_hex(s).ok())
            .unwrap_or(Fr::zero());

        // 3. Sync contract state for scopes
        // TODO: When ContractSyncService is fully wired with ACVM,
        // run sync for each contract referenced in the tx_request.

        // 4. Execute private functions via ACVM
        // TODO: When ACVM integration is complete, execute the private
        // functions from tx_request and collect PrivateCallExecution results.
        //
        // For now, we construct the kernel execution prover and delegate
        // to it. The actual ACVM execution is still pending Phase 1 completion.
        let _ = &self.kernel_prover;
        let _ = block_hash;
        let _ = &scopes;

        // 5. Run kernel circuit sequence: init → inner → reset → tail → hiding → chonk
        // This requires the ACVM execution results from step 4.
        // When complete, the flow will be:
        //
        // let execution_prover = PrivateKernelExecutionProver::from_stores(
        //     &self.node,
        //     &self.contract_store,
        //     &self.key_store,
        //     &self.kernel_prover,
        //     block_hash,
        //     KernelExecutionConfig::default(),
        // );
        // let result = execution_prover.prove_with_kernels(&executions).await?;
        //
        // 6. Store pre-tags in sender_tagging_store
        // 7. Assemble TxProvingResult

        // For now, return a structured error indicating the dependency on ACVM
        Err(Error::InvalidData(
            "prove_tx: private execution (ACVM) not yet integrated — \
             kernel prover infrastructure is ready but requires ACVM execution \
             results from Phase 1 to produce proofs"
                .into(),
        ))
    }

    async fn profile_tx(
        &self,
        _tx_request: &TxExecutionRequest,
        _opts: ProfileTxOpts,
    ) -> Result<TxProfileResult, Error> {
        // Phase 3
        Err(Error::InvalidData(
            "profile_tx not yet implemented (Phase 3)".into(),
        ))
    }

    async fn execute_utility(
        &self,
        _call: &FunctionCall,
        _opts: ExecuteUtilityOpts,
    ) -> Result<UtilityExecutionResult, Error> {
        // TODO(Phase 1): Execute unconstrained function via ACVM Brillig executor
        // with UtilityExecutionOracle. Requires acvm crate integration.
        Err(Error::InvalidData(
            "execute_utility requires ACVM integration (pending Phase 1 completion)".into(),
        ))
    }

    async fn get_private_events(
        &self,
        _event_selector: &EventSelector,
        _filter: PrivateEventFilter,
    ) -> Result<Vec<PackedPrivateEvent>, Error> {
        // Phase 3: event discovery
        Err(Error::InvalidData(
            "get_private_events not yet implemented (Phase 3)".into(),
        ))
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
    struct MockNode {
        registered_signatures: Mutex<Vec<String>>,
    }

    impl Default for MockNode {
        fn default() -> Self {
            Self {
                registered_signatures: Mutex::new(vec![]),
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
        async fn get_public_logs(
            &self,
            _filter: aztec_node_client::PublicLogFilter,
        ) -> Result<aztec_node_client::PublicLogsResponse, Error> {
            Ok(aztec_node_client::PublicLogsResponse {
                logs: vec![],
                max_logs_hit: false,
            })
        }
        async fn send_tx(&self, _tx: &serde_json::Value) -> Result<aztec_core::tx::TxHash, Error> {
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
        async fn is_valid_tx(&self, _tx: &serde_json::Value) -> Result<bool, Error> {
            Ok(true)
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
}
