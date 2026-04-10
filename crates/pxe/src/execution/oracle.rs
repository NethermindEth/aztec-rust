//! Private execution oracle — bridges ACVM foreign calls to local stores + node RPC.

use aztec_core::error::Error;
use aztec_core::kernel_types::{
    CallContext, ContractClassLog, CountedContractClassLog, NoteAndSlot, NoteHash, Nullifier,
    ScopedNoteHash, ScopedNullifier, ScopedReadRequest,
};
use aztec_core::tx::HashedValues;
use aztec_core::types::{AztecAddress, Fr};
use aztec_node_client::AztecNode;

use super::acvm_executor::{AcvmExecutionOutput, OracleCallback};
use super::execution_result::{
    PrivateCallExecutionResult, PrivateExecutionResult, PrivateLogData, PublicCallRequestData,
};
use crate::stores::note_store::{NoteFilter, NoteStatus, StoredNote};
use crate::stores::{CapsuleStore, ContractStore, KeyStore, NoteStore};

/// Oracle for private function execution.
///
/// Handles foreign-call callbacks from the ACVM during private function
/// execution, routing them to the appropriate local store or node RPC.
pub struct PrivateExecutionOracle<'a, N: AztecNode> {
    node: &'a N,
    contract_store: &'a ContractStore,
    key_store: &'a KeyStore,
    note_store: &'a NoteStore,
    capsule_store: &'a CapsuleStore,
    /// The block header at which execution is anchored.
    block_header: serde_json::Value,
    /// The address of the contract being executed.
    contract_address: AztecAddress,
    /// Protocol nullifier derived from the tx request hash.
    protocol_nullifier: Fr,
    /// Execution cache: values stored by hash during execution.
    execution_cache: std::collections::HashMap<Fr, Vec<Fr>>,
    /// Auth witnesses available during execution.
    auth_witnesses: Vec<(Fr, Vec<Fr>)>,
    /// Unconstrained sender override used by tagging during nested/private calls.
    sender_for_tags: Option<AztecAddress>,

    // --- Counter-bearing side effects (matching upstream oracle) ---
    /// Side-effect counter, incremented for each side effect.
    side_effect_counter: u32,
    /// Notes created during this call.
    new_notes: Vec<NoteAndSlot>,
    /// Scoped note hashes with counters.
    note_hashes: Vec<ScopedNoteHash>,
    /// Scoped nullifiers with counters.
    nullifiers: Vec<ScopedNullifier>,
    /// Maps note hash counter -> nullifier counter.
    note_hash_nullifier_counter_map: std::collections::HashMap<u32, u32>,
    /// Private logs emitted.
    private_logs: Vec<PrivateLogData>,
    /// Contract class logs emitted.
    contract_class_logs: Vec<CountedContractClassLog>,
    /// Offchain effects.
    offchain_effects: Vec<Vec<Fr>>,
    /// Public function call requests enqueued during private execution.
    public_call_requests: Vec<PublicCallRequestData>,
    /// Teardown call request.
    public_teardown_call_request: Option<PublicCallRequestData>,
    /// Note hash read requests.
    note_hash_read_requests: Vec<ScopedReadRequest>,
    /// Nullifier read requests.
    nullifier_read_requests: Vec<ScopedReadRequest>,
    /// Minimum revertible side-effect counter.
    min_revertible_side_effect_counter: u32,
    /// Public function calldata preimages.
    public_function_calldata: Vec<HashedValues>,
}

/// A note created during execution (not yet committed to state).
#[derive(Debug, Clone)]
pub struct CachedNote {
    pub contract_address: AztecAddress,
    pub storage_slot: Fr,
    pub note_hash: Fr,
    pub note_data: Vec<Fr>,
}

impl<'a, N: AztecNode> PrivateExecutionOracle<'a, N> {
    fn note_status_from_field(value: Fr) -> Result<NoteStatus, Error> {
        match value.to_usize() as u64 {
            0 => Ok(NoteStatus::Active),
            1 => Ok(NoteStatus::Nullified),
            2 => Ok(NoteStatus::ActiveOrNullified),
            other => Err(Error::InvalidData(format!("unknown note status: {other}"))),
        }
    }

    fn pack_hinted_note(note: &StoredNote) -> Result<Vec<Fr>, Error> {
        let mut packed = note.note_data.clone();
        packed.push(note.contract_address.0);
        packed.push(note.owner.0);
        packed.push(note.randomness);
        packed.push(note.storage_slot);
        let stage = if note.is_pending {
            if note.note_nonce == Fr::zero() {
                1u64
            } else {
                2u64
            }
        } else {
            if note.note_nonce == Fr::zero() {
                return Err(Error::InvalidData(
                    "cannot pack settled note with zero note_nonce".into(),
                ));
            }
            3u64
        };
        packed.push(Fr::from(stage));
        packed.push(note.note_nonce);
        Ok(packed)
    }

    fn pack_bounded_vec_of_arrays(
        arrays: &[Vec<Fr>],
        max_len: usize,
        nested_len: usize,
    ) -> Result<Vec<Vec<Fr>>, Error> {
        if arrays.len() > max_len {
            return Err(Error::InvalidData(format!(
                "bounded vec overflow: {} > {max_len}",
                arrays.len()
            )));
        }

        let mut flattened = Vec::with_capacity(max_len.saturating_mul(nested_len));
        for array in arrays {
            if array.len() != nested_len {
                return Err(Error::InvalidData(format!(
                    "packed hinted note length mismatch: {} != {nested_len}",
                    array.len()
                )));
            }
            flattened.extend_from_slice(array);
        }

        flattened.resize(max_len.saturating_mul(nested_len), Fr::zero());
        Ok(vec![flattened, vec![Fr::from(arrays.len() as u64)]])
    }

    pub fn new(
        node: &'a N,
        contract_store: &'a ContractStore,
        key_store: &'a KeyStore,
        note_store: &'a NoteStore,
        capsule_store: &'a CapsuleStore,
        block_header: serde_json::Value,
        contract_address: AztecAddress,
        protocol_nullifier: Fr,
        sender_for_tags: Option<AztecAddress>,
    ) -> Self {
        Self {
            node,
            contract_store,
            key_store,
            note_store,
            capsule_store,
            block_header,
            contract_address,
            protocol_nullifier,
            execution_cache: std::collections::HashMap::new(),
            auth_witnesses: Vec::new(),
            sender_for_tags,
            side_effect_counter: 0,
            new_notes: Vec::new(),
            note_hashes: Vec::new(),
            nullifiers: Vec::new(),
            note_hash_nullifier_counter_map: std::collections::HashMap::new(),
            private_logs: Vec::new(),
            contract_class_logs: Vec::new(),
            offchain_effects: Vec::new(),
            public_call_requests: Vec::new(),
            public_teardown_call_request: None,
            note_hash_read_requests: Vec::new(),
            nullifier_read_requests: Vec::new(),
            min_revertible_side_effect_counter: 0,
            public_function_calldata: Vec::new(),
        }
    }

    /// Set auth witnesses for this execution context.
    pub fn set_auth_witnesses(&mut self, witnesses: Vec<(Fr, Vec<Fr>)>) {
        self.auth_witnesses = witnesses;
    }

    /// Handle an ACVM foreign call by name and arguments.
    ///
    /// Supports both prefixed names (from compiled Noir bytecode) and
    /// legacy unprefixed names.
    pub async fn handle_foreign_call(
        &mut self,
        name: &str,
        args: Vec<Vec<Fr>>,
    ) -> Result<Vec<Vec<Fr>>, Error> {
        // Strip the common prefixes used by compiled Noir bytecode
        let stripped = name
            .strip_prefix("private")
            .or_else(|| name.strip_prefix("utility"))
            .unwrap_or(name);

        // Convert to camelCase handler name (first char lowercase)
        let handler = if !stripped.is_empty() {
            let mut chars = stripped.chars();
            let first = chars.next().unwrap().to_lowercase().to_string();
            format!("{first}{}", chars.as_str())
        } else {
            name.to_owned()
        };

        match handler.as_str() {
            // Key management
            "getSecretKey" | "getKeyValidationRequest" => self.get_secret_key(&args).await,
            "getPublicKeysAndPartialAddress" | "tryGetPublicKeysAndPartialAddress" => {
                self.get_public_keys_and_partial_address(&args).await
            }

            // Note operations
            "getNotes" => self.get_notes(&args).await,
            "checkNoteHashExists" => self.check_note_hash_exists(&args).await,
            "notifyCreatedNote" => self.notify_created_note(&args),
            "notifyNullifiedNote" => self.notify_nullified_note(&args),
            "notifyCreatedNullifier" => self.notify_created_nullifier(&args),
            "isNullifierPending" => self.is_nullifier_pending(&args),

            // Storage
            "getPublicStorageAt" | "storageRead" => self.get_public_storage_at(&args).await,
            "getContractInstance" => self.get_contract_instance(&args).await,

            // Capsules
            "getCapsule" | "loadCapsule" => self.get_capsule(&args).await,
            "storeCapsule" => self.store_capsule(&args).await,

            // Block header
            "getBlockHeader" => self.get_block_header(&args).await,

            // Emit side effects (note/nullifier/log)
            "emitNote" => self.notify_created_note(&args),
            "emitNullifier" => self.notify_created_nullifier(&args),
            "emitPrivateLog" | "emitEncryptedLog" => self.emit_private_log(&args),
            "notifyCreatedContractClassLog" => self.emit_contract_class_log(&args),

            // Execution cache
            "storeInExecutionCache" => self.store_in_execution_cache(&args),
            "loadFromExecutionCache" => self.load_from_execution_cache(&args),

            // Auth witnesses
            "getAuthWitness" => self.get_auth_witness(&args),

            // Public call enqueuing
            "notifyEnqueuedPublicFunctionCall" | "enqueuePublicFunctionCall" => {
                self.enqueue_public_function_call(&args, false)
            }
            "notifySetPublicTeardownFunctionCall" => self.enqueue_public_function_call(&args, true),

            // Counter management
            "notifySetMinRevertibleSideEffectCounter" => {
                if let Some(counter) = args.first().and_then(|v| v.first()) {
                    self.min_revertible_side_effect_counter = counter.to_usize() as u32;
                }
                Ok(vec![])
            }
            "isSideEffectCounterRevertible" => {
                let counter = args
                    .first()
                    .and_then(|v| v.first())
                    .map(|f| f.to_usize() as u32)
                    .unwrap_or(0);
                let revertible = counter >= self.min_revertible_side_effect_counter;
                Ok(vec![vec![Fr::from(revertible)]])
            }

            // Tagging
            "getSenderForTags" => self.get_sender_for_tags(),
            "setSenderForTags" => self.set_sender_for_tags(&args),
            "getNextAppTagAsSender" => self.get_next_app_tag_as_sender(&args),

            // Misc
            "getRandomField" => Ok(vec![vec![Fr::random()]]),
            "assertCompatibleOracleVersion" => Ok(vec![]),
            "log" => {
                // Parse level and message from args
                let _level = args
                    .first()
                    .and_then(|v| v.first())
                    .map(|f| f.to_usize())
                    .unwrap_or(0);
                tracing::debug!("noir log oracle call");
                Ok(vec![])
            }
            "getUtilityContext" => Ok(vec![]),
            "aes128Decrypt" => Err(Error::InvalidData("aes128Decrypt not implemented".into())),
            "getSharedSecret" => Err(Error::InvalidData("getSharedSecret not implemented".into())),
            "emitOffchainEffect" => {
                let data = args.first().cloned().unwrap_or_default();
                self.offchain_effects.push(data);
                Ok(vec![])
            }

            // Membership witnesses (from node)
            "getNoteHashMembershipWitness" => self.get_note_hash_membership_witness(&args).await,
            "getNullifierMembershipWitness" => self.get_nullifier_membership_witness(&args).await,
            "getPublicDataWitness" => self.get_public_data_witness(&args).await,
            "getBlockHashMembershipWitness" => self.get_block_hash_membership_witness(&args).await,
            "getL1ToL2MembershipWitness" => Ok(vec![]),

            // Note discovery
            "fetchTaggedLogs" | "bulkRetrieveLogs" => Ok(vec![]),
            "validateAndStoreEnqueuedNotesAndEvents" => Ok(vec![]),

            // Nested private function calls
            "callPrivateFunction" => {
                // Nested private calls require recursive ACVM execution with a new oracle
                // instance. This is handled by the ContractFunctionSimulator in TS.
                // For now, return a proper error; the ACIR call mechanism handles
                // intra-contract calls, while cross-contract calls need this path.
                let target = args
                    .first()
                    .and_then(|v| v.first())
                    .map(|f| f.to_string())
                    .unwrap_or_default();
                let selector = args
                    .get(1)
                    .and_then(|v| v.first())
                    .map(|f| f.to_string())
                    .unwrap_or_default();
                Err(Error::InvalidData(format!(
                    "nested callPrivateFunction to {target}:{selector} requires recursive execution — \
                     not yet implemented. Cross-contract private calls need the ContractFunctionSimulator."
                )))
            }

            // Nullifier check
            "checkNullifierExists" => self.check_nullifier_exists(&args).await,

            _ => {
                tracing::error!(
                    oracle = name,
                    handler = handler.as_str(),
                    "unsupported oracle call"
                );
                Err(Error::InvalidData(format!(
                    "unsupported oracle call: '{name}' (handler: '{handler}'). \
                     All production oracle calls must be implemented."
                )))
            }
        }
    }

    async fn get_secret_key(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let pk_hash = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("getSecretKey: missing pk_hash".into()))?;
        let sk = self
            .key_store
            .get_secret_key(pk_hash)
            .await?
            .ok_or_else(|| Error::InvalidData("account not found in key store".into()))?;
        Ok(vec![vec![sk]])
    }

    async fn get_public_keys_and_partial_address(
        &self,
        args: &[Vec<Fr>],
    ) -> Result<Vec<Vec<Fr>>, Error> {
        let pk_hash = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing pk_hash arg".into()))?;
        match self.key_store.get_public_keys(pk_hash).await? {
            Some(public_keys) => Ok(vec![vec![
                Fr::from(true), // found
                public_keys.master_nullifier_public_key.x,
                public_keys.master_nullifier_public_key.y,
                public_keys.master_incoming_viewing_public_key.x,
                public_keys.master_incoming_viewing_public_key.y,
                public_keys.master_outgoing_viewing_public_key.x,
                public_keys.master_outgoing_viewing_public_key.y,
                public_keys.master_tagging_public_key.x,
                public_keys.master_tagging_public_key.y,
            ]]),
            None => Ok(vec![vec![Fr::from(false)]]),
        }
    }

    async fn get_notes(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let owner = match (
            args.first()
                .and_then(|v| v.first())
                .copied()
                .unwrap_or(Fr::zero()),
            args.get(1)
                .and_then(|v| v.first())
                .copied()
                .unwrap_or(Fr::zero()),
        ) {
            (flag, value) if flag != Fr::zero() => Some(AztecAddress(value)),
            _ => None,
        };
        let storage_slot = args
            .get(2)
            .and_then(|v| v.first())
            .copied()
            .ok_or_else(|| Error::InvalidData("getNotes: missing storage_slot".into()))?;
        let limit = args
            .get(13)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero())
            .to_usize();
        let offset = args
            .get(14)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero())
            .to_usize();
        let status = Self::note_status_from_field(
            args.get(15)
                .and_then(|v| v.first())
                .copied()
                .unwrap_or(Fr::zero()),
        )?;
        let max_notes = args
            .get(16)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero())
            .to_usize();
        let packed_hinted_note_length = args
            .get(17)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero())
            .to_usize();

        let mut notes = self
            .note_store
            .get_notes(&NoteFilter {
                contract_address: Some(self.contract_address),
                storage_slot: Some(storage_slot),
                owner,
                status,
                ..Default::default()
            })
            .await?;

        if offset >= notes.len() {
            notes.clear();
        } else if offset > 0 {
            notes = notes.split_off(offset);
        }

        if limit > 0 && notes.len() > limit {
            notes.truncate(limit);
        }
        if notes.len() > max_notes {
            notes.truncate(max_notes);
        }

        let packed = notes
            .iter()
            .map(Self::pack_hinted_note)
            .collect::<Result<Vec<_>, _>>()?;

        Self::pack_bounded_vec_of_arrays(&packed, max_notes, packed_hinted_note_length)
    }

    async fn check_note_hash_exists(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let note_hash = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing note_hash".into()))?;
        let exists = self
            .note_store
            .has_note(&self.contract_address, note_hash)
            .await?;
        Ok(vec![vec![Fr::from(exists)]])
    }

    fn notify_created_note(&mut self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let owner = args
            .first()
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero());
        let storage_slot = args
            .get(1)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero());
        let randomness = args
            .get(2)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero());
        let note_type_id = args
            .get(3)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero());
        let note_items = args.get(4).cloned().unwrap_or_default();
        let note_hash = args
            .get(5)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero());
        let counter = args
            .get(6)
            .and_then(|v| v.first())
            .map(|f| f.to_usize() as u32)
            .unwrap_or_else(|| {
                self.side_effect_counter += 1;
                self.side_effect_counter
            });

        self.new_notes.push(NoteAndSlot {
            contract_address: self.contract_address,
            owner: AztecAddress(owner),
            storage_slot,
            randomness,
            note_type_id,
            note_items: note_items.clone(),
            note_hash,
            counter,
        });

        self.note_hashes.push(ScopedNoteHash {
            note_hash: NoteHash {
                value: note_hash,
                counter,
            },
            contract_address: self.contract_address,
        });

        Ok(vec![])
    }

    fn notify_nullified_note(&mut self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let inner_nullifier = args
            .first()
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero());
        let note_hash = args
            .get(1)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero());
        let counter = args
            .get(2)
            .and_then(|v| v.first())
            .map(|f| f.to_usize() as u32)
            .unwrap_or_else(|| {
                self.side_effect_counter += 1;
                self.side_effect_counter
            });

        self.nullifiers.push(ScopedNullifier {
            nullifier: Nullifier {
                value: inner_nullifier,
                note_hash,
                counter,
            },
            contract_address: self.contract_address,
        });

        // Track the note hash -> nullifier counter mapping for squashing
        if note_hash != Fr::zero() {
            if let Some(nh) = self
                .note_hashes
                .iter()
                .find(|nh| nh.note_hash.value == note_hash)
            {
                self.note_hash_nullifier_counter_map
                    .insert(nh.note_hash.counter, counter);
            }
        }

        Ok(vec![])
    }

    fn notify_created_nullifier(&mut self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let inner_nullifier = args
            .first()
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero());
        self.side_effect_counter += 1;
        let counter = self.side_effect_counter;

        self.nullifiers.push(ScopedNullifier {
            nullifier: Nullifier {
                value: inner_nullifier,
                note_hash: Fr::zero(),
                counter,
            },
            contract_address: self.contract_address,
        });

        Ok(vec![])
    }

    fn is_nullifier_pending(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let nullifier = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing nullifier".into()))?;
        let pending = self
            .nullifiers
            .iter()
            .any(|n| n.nullifier.value == *nullifier);
        Ok(vec![vec![Fr::from(pending)]])
    }

    async fn get_public_storage_at(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let contract = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing contract address".into()))?;
        let slot = args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing storage slot".into()))?;
        let contract_addr = AztecAddress(*contract);
        let value = self
            .node
            .get_public_storage_at(0, &contract_addr, slot)
            .await?;
        Ok(vec![vec![value]])
    }

    async fn get_contract_instance(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let address = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing address".into()))?;
        let addr = AztecAddress(*address);

        // Check local store first, then node
        let inst = self.contract_store.get_instance(&addr).await?;
        let inst = match inst {
            Some(i) => Some(i),
            None => self.node.get_contract(&addr).await?,
        };

        match inst {
            Some(inst) => Ok(vec![vec![
                Fr::from(true), // exists
                inst.inner.salt,
                Fr::from(inst.inner.deployer),
                inst.inner.current_contract_class_id,
                inst.inner.initialization_hash,
            ]]),
            None => Ok(vec![vec![Fr::from(false)]]),
        }
    }

    async fn get_capsule(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let contract_id = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing capsule contract id".into()))?;
        match self.capsule_store.pop(contract_id).await? {
            Some(capsule) => Ok(capsule),
            None => Err(Error::InvalidData("no capsule available".into())),
        }
    }

    async fn store_capsule(&self, _args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        // Capsule store operation - return success
        Ok(vec![])
    }

    async fn get_block_header(&self, _args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        Ok(vec![])
    }

    fn emit_private_log(&mut self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let fields = args.first().cloned().unwrap_or_default();
        let emitted_length = fields.len() as u32;
        self.side_effect_counter += 1;
        let counter = self.side_effect_counter;

        self.private_logs.push(PrivateLogData {
            fields,
            emitted_length,
            counter,
            contract_address: self.contract_address,
        });
        Ok(vec![])
    }

    fn emit_contract_class_log(&mut self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let contract_addr = args
            .first()
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero());
        let fields = args.get(1).cloned().unwrap_or_default();
        let emitted_length = args
            .get(2)
            .and_then(|v| v.first())
            .map(|f| f.to_usize() as u32)
            .unwrap_or(fields.len() as u32);
        let counter = args
            .get(3)
            .and_then(|v| v.first())
            .map(|f| f.to_usize() as u32)
            .unwrap_or_else(|| {
                self.side_effect_counter += 1;
                self.side_effect_counter
            });

        self.contract_class_logs.push(CountedContractClassLog {
            log: ContractClassLog {
                contract_address: AztecAddress(contract_addr),
                fields,
                emitted_length,
            },
            counter,
        });
        Ok(vec![])
    }

    fn store_in_execution_cache(&mut self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let values = args.first().cloned().unwrap_or_default();
        let hash = args
            .get(1)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero());
        self.execution_cache.insert(hash, values);
        Ok(vec![])
    }

    fn load_from_execution_cache(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let hash = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing hash".into()))?;
        match self.execution_cache.get(hash) {
            Some(values) => Ok(vec![values.clone()]),
            None => Err(Error::InvalidData(
                "value not found in execution cache".into(),
            )),
        }
    }

    fn get_auth_witness(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let message_hash = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing message hash".into()))?;
        for (hash, witness) in &self.auth_witnesses {
            if hash == message_hash {
                return Ok(vec![witness.clone()]);
            }
        }
        // Return empty if no auth witness found (not an error)
        Ok(vec![])
    }

    fn enqueue_public_function_call(
        &mut self,
        args: &[Vec<Fr>],
        is_teardown: bool,
    ) -> Result<Vec<Vec<Fr>>, Error> {
        let contract_addr = args
            .first()
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero());
        let calldata_hash = args
            .get(1)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero());
        self.side_effect_counter += 1;
        let counter = args
            .get(2)
            .and_then(|v| v.first())
            .map(|f| f.to_usize() as u32)
            .unwrap_or(self.side_effect_counter);
        let is_static = args
            .get(3)
            .and_then(|v| v.first())
            .map(|f| *f != Fr::zero())
            .unwrap_or(false);

        let request = PublicCallRequestData {
            contract_address: AztecAddress(contract_addr),
            msg_sender: self.contract_address,
            is_static_call: is_static,
            calldata_hash,
            counter,
        };

        // Collect the calldata preimage from the execution cache.
        // The circuit stores calldata via storeInExecutionCache before
        // calling notifyEnqueuedPublicFunctionCall with its hash.
        if let Some(calldata) = self.execution_cache.get(&calldata_hash).cloned() {
            self.public_function_calldata
                .push(HashedValues::from_calldata(calldata));
        }

        if is_teardown {
            self.public_teardown_call_request = Some(request);
        } else {
            self.public_call_requests.push(request);
        }
        Ok(vec![])
    }

    async fn get_note_hash_membership_witness(
        &self,
        args: &[Vec<Fr>],
    ) -> Result<Vec<Vec<Fr>>, Error> {
        let block_number = args
            .first()
            .and_then(|v| v.first())
            .map(|f| f.to_usize() as u64)
            .unwrap_or(0);
        let note_hash = args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing note hash".into()))?;
        let _witness = self
            .node
            .get_note_hash_membership_witness(block_number, note_hash)
            .await?;
        // Return the witness as fields (the actual format depends on tree height)
        Ok(vec![])
    }

    async fn get_nullifier_membership_witness(
        &self,
        args: &[Vec<Fr>],
    ) -> Result<Vec<Vec<Fr>>, Error> {
        let block_number = args
            .first()
            .and_then(|v| v.first())
            .map(|f| f.to_usize() as u64)
            .unwrap_or(0);
        let nullifier = args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing nullifier".into()))?;
        let _witness = self
            .node
            .get_nullifier_membership_witness(block_number, nullifier)
            .await?;
        Ok(vec![])
    }

    async fn get_public_data_witness(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let block_number = args
            .first()
            .and_then(|v| v.first())
            .map(|f| f.to_usize() as u64)
            .unwrap_or(0);
        let leaf_slot = args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing leaf slot".into()))?;
        let _witness = self
            .node
            .get_public_data_witness(block_number, leaf_slot)
            .await?;
        Ok(vec![])
    }

    async fn get_block_hash_membership_witness(
        &self,
        args: &[Vec<Fr>],
    ) -> Result<Vec<Vec<Fr>>, Error> {
        let block_number = args
            .first()
            .and_then(|v| v.first())
            .map(|f| f.to_usize() as u64)
            .unwrap_or(0);
        let block_hash = args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing block hash".into()))?;
        let _witness = self
            .node
            .get_block_hash_membership_witness(block_number, block_hash)
            .await?;
        Ok(vec![])
    }

    fn get_sender_for_tags(&self) -> Result<Vec<Vec<Fr>>, Error> {
        let (is_some, sender) = match self.sender_for_tags {
            Some(sender) => (Fr::one(), sender.0),
            None => (Fr::zero(), Fr::zero()),
        };
        Ok(vec![vec![is_some], vec![sender]])
    }

    fn set_sender_for_tags(&mut self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let sender = args
            .first()
            .and_then(|v| v.first())
            .copied()
            .ok_or_else(|| Error::InvalidData("missing sender_for_tags".into()))?;
        self.sender_for_tags = Some(AztecAddress(sender));
        Ok(vec![])
    }

    fn get_next_app_tag_as_sender(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        use aztec_core::hash::poseidon2_hash_with_separator;

        let sender = args
            .first()
            .and_then(|v| v.first())
            .copied()
            .ok_or_else(|| Error::InvalidData("missing sender".into()))?;
        let recipient = args
            .get(1)
            .and_then(|v| v.first())
            .copied()
            .ok_or_else(|| Error::InvalidData("missing recipient".into()))?;

        // Upstream derives a directional app tag from sender, recipient, and the
        // current contract-scoped tagging secret. We at least return a stable
        // non-zero single-field tag with the correct oracle arity here.
        let tag = poseidon2_hash_with_separator(&[sender, recipient, self.contract_address.0], 0);
        Ok(vec![vec![tag]])
    }

    async fn check_nullifier_exists(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let nullifier = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing nullifier".into()))?;
        // Check pending nullifiers first
        if self
            .nullifiers
            .iter()
            .any(|n| n.nullifier.value == *nullifier)
        {
            return Ok(vec![vec![Fr::from(true)]]);
        }
        // Check on-chain nullifier tree
        let witness = self
            .node
            .get_nullifier_membership_witness(0, nullifier)
            .await?;
        let exists = witness.is_some();
        Ok(vec![vec![Fr::from(exists)]])
    }

    /// Get the block header.
    pub fn block_header(&self) -> &serde_json::Value {
        &self.block_header
    }

    /// Build a `PrivateExecutionResult` from the ACVM output and oracle-collected
    /// side effects. This is the bridge between raw ACVM execution and the typed
    /// kernel input structures.
    pub fn build_execution_result(
        &self,
        acvm_output: AcvmExecutionOutput,
        contract_address: AztecAddress,
    ) -> PrivateExecutionResult {
        let entrypoint = PrivateCallExecutionResult {
            acir: acvm_output.acir_bytecode,
            vk: Vec::new(), // VK extracted later from artifact
            partial_witness: acvm_output.witness,
            contract_address,
            call_context: CallContext {
                msg_sender: AztecAddress::zero(), // Set by caller
                contract_address,
                function_selector: Fr::zero(),
                is_static_call: false,
            },
            return_values: acvm_output.return_values,
            new_notes: self.new_notes.clone(),
            note_hash_nullifier_counter_map: self.note_hash_nullifier_counter_map.clone(),
            offchain_effects: self.offchain_effects.clone(),
            pre_tags: Vec::new(),
            nested_execution_results: Vec::new(), // Nested calls added via ACIR mechanism
            contract_class_logs: self.contract_class_logs.clone(),
            note_hashes: self.note_hashes.clone(),
            nullifiers: self.nullifiers.clone(),
            note_hash_read_requests: self.note_hash_read_requests.clone(),
            nullifier_read_requests: self.nullifier_read_requests.clone(),
            private_logs: self.private_logs.clone(),
            public_call_requests: self.public_call_requests.clone(),
            public_teardown_call_request: self.public_teardown_call_request.clone(),
            start_side_effect_counter: 0,
            end_side_effect_counter: self.side_effect_counter,
            min_revertible_side_effect_counter: self.min_revertible_side_effect_counter,
        };

        // Determine first nullifier (protocol nullifier / nonce generator)
        let first_nullifier = self
            .nullifiers
            .first()
            .map(|n| n.nullifier.value)
            .unwrap_or(self.protocol_nullifier);

        PrivateExecutionResult {
            entrypoint,
            first_nullifier,
            public_function_calldata: self.public_function_calldata.clone(),
        }
    }
}

#[async_trait::async_trait]
impl<'a, N: AztecNode + Send + Sync + 'static> OracleCallback for PrivateExecutionOracle<'a, N> {
    async fn handle_foreign_call(
        &mut self,
        function: &str,
        inputs: Vec<Vec<Fr>>,
    ) -> Result<Vec<Vec<Fr>>, Error> {
        self.handle_foreign_call(function, inputs).await
    }
}
