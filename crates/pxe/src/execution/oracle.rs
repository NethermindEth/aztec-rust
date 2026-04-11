//! Private execution oracle — bridges ACVM foreign calls to local stores + node RPC.

use aztec_core::error::Error;
use aztec_core::kernel_types::{
    CallContext, ContractClassLog, CountedContractClassLog, NoteAndSlot, NoteHash, Nullifier,
    ScopedNoteHash, ScopedNullifier, ScopedReadRequest,
};
use aztec_core::tx::HashedValues;
use aztec_core::types::{AztecAddress, ContractInstance, Fr};
use aztec_node_client::AztecNode;

use super::acvm_executor::{AcvmExecutionOutput, OracleCallback};
use super::execution_result::{
    PrivateCallExecutionResult, PrivateExecutionResult, PrivateLogData, PublicCallRequestData,
};
use crate::stores::note_store::{NoteFilter, NoteStatus, StoredNote};
use crate::stores::{
    AddressStore, CapsuleStore, ContractStore, KeyStore, NoteStore, SenderTaggingStore,
};

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
    address_store: &'a AddressStore,
    sender_tagging_store: &'a SenderTaggingStore,
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
    /// Execution scopes — used to enforce key validation access control.
    scopes: Vec<AztecAddress>,
    /// Whether the currently executing private function is in a static context.
    call_is_static: bool,

    // --- Counter-bearing side effects (matching upstream oracle) ---
    /// Side-effect counter, incremented for each side effect.
    pub(crate) side_effect_counter: u32,
    /// Notes created during this call.
    pub(crate) new_notes: Vec<NoteAndSlot>,
    /// Scoped note hashes with counters.
    pub(crate) note_hashes: Vec<ScopedNoteHash>,
    /// Scoped nullifiers with counters.
    pub(crate) nullifiers: Vec<ScopedNullifier>,
    /// Maps note hash counter -> nullifier counter.
    pub(crate) note_hash_nullifier_counter_map: std::collections::HashMap<u32, u32>,
    /// Siloed nullifier values of DB notes consumed during this execution.
    /// Used to prevent returning already-consumed persistent notes from get_notes.
    consumed_db_nullifiers: std::collections::HashSet<Fr>,
    /// Private logs emitted.
    pub(crate) private_logs: Vec<PrivateLogData>,
    /// Contract class logs emitted.
    pub(crate) contract_class_logs: Vec<CountedContractClassLog>,
    /// Offchain effects.
    offchain_effects: Vec<Vec<Fr>>,
    /// Public function call requests enqueued during private execution.
    public_call_requests: Vec<PublicCallRequestData>,
    /// Teardown call request.
    public_teardown_call_request: Option<PublicCallRequestData>,
    /// Note hash read requests.
    pub(crate) note_hash_read_requests: Vec<ScopedReadRequest>,
    /// Nullifier read requests.
    pub(crate) nullifier_read_requests: Vec<ScopedReadRequest>,
    /// Minimum revertible side-effect counter.
    pub(crate) min_revertible_side_effect_counter: u32,
    /// Public function calldata preimages.
    public_function_calldata: Vec<HashedValues>,
    /// Captured nested execution results (for return value extraction).
    pub(crate) nested_results: Vec<PrivateCallExecutionResult>,
    /// Block-header + tx-context fields from the entrypoint witness,
    /// shared with nested calls so that chain_id/version are correct.
    pub(crate) context_witness_prefix: Vec<Fr>,
}

fn decode_base64_sibling_path(encoded: &str) -> Result<Vec<Fr>, Error> {
    use base64::Engine;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| Error::InvalidData(format!("invalid siblingPath base64: {e}")))?;

    let payload = if bytes.len() >= 4 {
        let declared_len =
            u32::from_be_bytes(bytes[..4].try_into().expect("length prefix is 4 bytes")) as usize;
        let payload = &bytes[4..];
        if payload.len() == declared_len.saturating_mul(32) {
            payload
        } else if bytes.len() % 32 == 0 {
            bytes.as_slice()
        } else {
            return Err(Error::InvalidData(format!(
                "siblingPath payload length mismatch: declared {declared_len} elements, got {} bytes",
                payload.len()
            )));
        }
    } else {
        bytes.as_slice()
    };

    Ok(payload
        .chunks(32)
        .map(|chunk| {
            let mut padded = [0u8; 32];
            let start = 32usize.saturating_sub(chunk.len());
            padded[start..].copy_from_slice(chunk);
            Fr::from(padded)
        })
        .collect())
}

fn parse_field_string(value: &str) -> Result<Fr, Error> {
    if value.starts_with("0x") {
        Fr::from_hex(value)
    } else {
        value
            .parse::<u128>()
            .map(Fr::from)
            .map_err(|_| Error::InvalidData(format!("unsupported field string value: {value}")))
    }
}

/// A note created during execution (not yet committed to state).
#[derive(Debug, Clone)]
pub struct CachedNote {
    pub contract_address: AztecAddress,
    pub storage_slot: Fr,
    pub note_hash: Fr,
    pub note_data: Vec<Fr>,
}

impl<'a, N: AztecNode + 'static> PrivateExecutionOracle<'a, N> {
    /// Extract circuit-constrained side effects from a solved ACVM witness.
    ///
    /// Private logs, note hashes, and nullifiers are NOT emitted via oracle calls;
    /// they are circuit outputs embedded in the witness at known PCPI offsets.
    fn extract_side_effects_from_witness(
        witness: &acir::native_types::WitnessMap<acir::FieldElement>,
        params_size: usize,
        contract_address: AztecAddress,
    ) -> (
        Vec<aztec_core::kernel_types::ScopedNoteHash>,
        Vec<aztec_core::kernel_types::ScopedNullifier>,
        Vec<PrivateLogData>,
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
                .map(|fe| super::field_conversion::fe_to_fr(fe))
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
                logs.push(PrivateLogData {
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
}

impl<'a, N: AztecNode + 'static> PrivateExecutionOracle<'a, N> {
    fn merge_nested_private_logs(
        nested_logs: Vec<PrivateLogData>,
        circuit_logs: Vec<PrivateLogData>,
    ) -> Vec<PrivateLogData> {
        if circuit_logs.is_empty() {
            return nested_logs;
        }

        let mut merged = nested_logs;
        for circuit_log in circuit_logs {
            if let Some(existing) = merged
                .iter_mut()
                .find(|log| log.counter == circuit_log.counter)
            {
                *existing = circuit_log;
            } else {
                merged.push(circuit_log);
            }
        }
        merged
    }

    fn try_handle_protocol_nested_private_call(
        &mut self,
        target_address: AztecAddress,
        selector: aztec_core::abi::FunctionSelector,
        encoded_args: &[Fr],
        circuit_side_effect_counter: u32,
        is_static: bool,
    ) -> Result<Option<Vec<Vec<Fr>>>, Error> {
        if is_static {
            return Ok(None);
        }

        let publish_instance_selector = aztec_core::abi::FunctionSelector::from_signature(
            "publish_for_public_execution(Field,(Field),Field,(((Field,Field,bool)),((Field,Field,bool)),((Field,Field,bool)),((Field,Field,bool))),bool)",
        );

        if target_address
            != aztec_core::constants::protocol_contract_address::contract_instance_registry()
            || selector != publish_instance_selector
        {
            return Ok(None);
        }

        if encoded_args.len() < 16 {
            return Err(Error::InvalidData(format!(
                "nested publish_for_public_execution args too short: {}",
                encoded_args.len()
            )));
        }

        let salt = encoded_args[0];
        let class_id = encoded_args[1];
        let initialization_hash = encoded_args[2];
        let public_keys = aztec_core::types::PublicKeys {
            master_nullifier_public_key: aztec_core::types::Point {
                x: encoded_args[3],
                y: encoded_args[4],
                is_infinite: encoded_args[5] != Fr::zero(),
            },
            master_incoming_viewing_public_key: aztec_core::types::Point {
                x: encoded_args[6],
                y: encoded_args[7],
                is_infinite: encoded_args[8] != Fr::zero(),
            },
            master_outgoing_viewing_public_key: aztec_core::types::Point {
                x: encoded_args[9],
                y: encoded_args[10],
                is_infinite: encoded_args[11] != Fr::zero(),
            },
            master_tagging_public_key: aztec_core::types::Point {
                x: encoded_args[12],
                y: encoded_args[13],
                is_infinite: encoded_args[14] != Fr::zero(),
            },
        };
        let universal_deploy = encoded_args[15] != Fr::zero();
        let origin = self.sender_for_tags.unwrap_or(self.contract_address);
        let deployer = if universal_deploy {
            AztecAddress::zero()
        } else {
            origin
        };

        let inner = ContractInstance {
            version: 1,
            salt,
            deployer,
            current_contract_class_id: class_id,
            original_contract_class_id: class_id,
            initialization_hash,
            public_keys: public_keys.clone(),
        };
        let instance_address = aztec_core::hash::compute_contract_address_from_instance(&inner)?;

        let event_payload = vec![
            aztec_core::constants::contract_instance_published_magic_value(),
            instance_address.0,
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
        let mut emitted_private_log_fields = event_payload;
        emitted_private_log_fields.push(Fr::zero());

        let nullifier_counter = circuit_side_effect_counter;
        let private_log_counter = nullifier_counter.saturating_add(1);
        let end_side_effect_counter = private_log_counter.saturating_add(1);

        self.nullifiers.push(ScopedNullifier {
            nullifier: Nullifier {
                value: instance_address.0,
                note_hash: Fr::zero(),
                counter: nullifier_counter,
            },
            contract_address: target_address,
        });
        self.private_logs.push(PrivateLogData {
            fields: emitted_private_log_fields,
            emitted_length: 15,
            note_hash_counter: 0,
            counter: private_log_counter,
            contract_address: target_address,
        });
        self.side_effect_counter = self.side_effect_counter.max(end_side_effect_counter);

        let returns_hash = aztec_core::hash::compute_var_args_hash(&[]);
        self.execution_cache.entry(returns_hash).or_default();
        self.nested_results.push(PrivateCallExecutionResult {
            contract_address: target_address,
            call_context: CallContext {
                msg_sender: self.contract_address,
                contract_address: target_address,
                function_selector: selector.to_field(),
                is_static_call: false,
            },
            start_side_effect_counter: nullifier_counter,
            end_side_effect_counter,
            min_revertible_side_effect_counter: nullifier_counter,
            ..Default::default()
        });

        Ok(Some(vec![vec![
            Fr::from(u64::from(end_side_effect_counter)),
            returns_hash,
        ]]))
    }

    /// Map Noir NoteStatus enum values: ACTIVE = 1, ACTIVE_OR_NULLIFIED = 2.
    fn note_status_from_field(value: Fr) -> Result<NoteStatus, Error> {
        match value.to_usize() as u64 {
            1 => Ok(NoteStatus::Active),
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
        address_store: &'a AddressStore,
        sender_tagging_store: &'a SenderTaggingStore,
        block_header: serde_json::Value,
        contract_address: AztecAddress,
        protocol_nullifier: Fr,
        sender_for_tags: Option<AztecAddress>,
        scopes: Vec<AztecAddress>,
        call_is_static: bool,
    ) -> Self {
        Self {
            node,
            contract_store,
            key_store,
            note_store,
            capsule_store,
            address_store,
            sender_tagging_store,
            block_header,
            contract_address,
            protocol_nullifier,
            execution_cache: std::collections::HashMap::new(),
            auth_witnesses: Vec::new(),
            sender_for_tags,
            scopes,
            call_is_static,
            side_effect_counter: 0,
            new_notes: Vec::new(),
            note_hashes: Vec::new(),
            nullifiers: Vec::new(),
            note_hash_nullifier_counter_map: std::collections::HashMap::new(),
            consumed_db_nullifiers: std::collections::HashSet::new(),
            private_logs: Vec::new(),
            contract_class_logs: Vec::new(),
            offchain_effects: Vec::new(),
            public_call_requests: Vec::new(),
            public_teardown_call_request: None,
            note_hash_read_requests: Vec::new(),
            nullifier_read_requests: Vec::new(),
            min_revertible_side_effect_counter: 0,
            public_function_calldata: Vec::new(),
            nested_results: Vec::new(),
            context_witness_prefix: Vec::new(),
        }
    }

    fn ensure_mutable_context(&self) -> Result<(), Error> {
        if self.call_is_static {
            return Err(Error::InvalidData(
                "Static call cannot update the state".into(),
            ));
        }
        Ok(())
    }

    /// Set auth witnesses for this execution context.
    pub fn set_auth_witnesses(&mut self, witnesses: Vec<(Fr, Vec<Fr>)>) {
        self.auth_witnesses = witnesses;
    }

    /// Pre-populate the execution cache with hashed values from the tx request.
    ///
    /// Mirrors the TS SDK's `HashedValuesCache.create(request.argsOfCalls)`.
    /// The Noir entrypoint calls `loadFromExecutionCache(hash)` to retrieve
    /// the args for each nested call; without pre-seeding the cache these
    /// lookups would fail.
    pub fn seed_execution_cache(&mut self, hashed_values: &[aztec_core::tx::HashedValues]) {
        for hv in hashed_values {
            self.execution_cache.insert(hv.hash, hv.values.clone());
        }
    }

    /// Return the public call requests accumulated during this execution.
    pub fn take_public_call_requests(
        &mut self,
    ) -> Vec<crate::execution::execution_result::PublicCallRequestData> {
        std::mem::take(&mut self.public_call_requests)
    }

    /// Return the public function calldata accumulated during this execution.
    pub fn take_public_function_calldata(&mut self) -> Vec<aztec_core::tx::HashedValues> {
        std::mem::take(&mut self.public_function_calldata)
    }

    /// Return the teardown call request if one was enqueued.
    pub fn take_teardown_call_request(
        &mut self,
    ) -> Option<crate::execution::execution_result::PublicCallRequestData> {
        self.public_teardown_call_request.take()
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
            "getNextAppTagAsSender" => self.get_next_app_tag_as_sender(&args).await,

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
            "callPrivateFunction" => self.call_private_function(&args).await,

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

    /// Return `KeyValidationRequest { pk_m: Point, sk_app: Field }` (4 fields).
    /// Uses pk_m_hash to find the right key type across all accounts.
    ///
    /// Enforces scope isolation: only keys belonging to accounts in the
    /// current execution scopes are accessible.
    async fn get_secret_key(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        use aztec_core::hash::poseidon2_hash;

        let pk_m_hash = *args.first().and_then(|v| v.first()).ok_or_else(|| {
            Error::InvalidData("getKeyValidationRequest: missing pk_m_hash".into())
        })?;

        // Check scope: ensure the key owner is within the current scopes
        if !self.scopes.is_empty() {
            let mut key_in_scope = false;
            for scope in &self.scopes {
                if let Some(complete) = self.address_store.get(scope).await? {
                    let pk = &complete.public_keys;
                    for point in [
                        &pk.master_nullifier_public_key,
                        &pk.master_incoming_viewing_public_key,
                        &pk.master_outgoing_viewing_public_key,
                        &pk.master_tagging_public_key,
                    ] {
                        let hash = poseidon2_hash(&[point.x, point.y, Fr::from(point.is_infinite)]);
                        if hash == pk_m_hash {
                            key_in_scope = true;
                            break;
                        }
                    }
                    if key_in_scope {
                        break;
                    }
                }
            }
            if !key_in_scope {
                return Err(Error::InvalidData("Key validation request denied".into()));
            }
        }

        match self
            .key_store
            .get_key_validation_request(&pk_m_hash, &self.contract_address)
            .await?
        {
            Some((pk_m, sk_app)) => Ok(vec![
                vec![pk_m.x],
                vec![pk_m.y],
                vec![Fr::from(pk_m.is_infinite)],
                vec![sk_app],
            ]),
            None => Ok(vec![
                vec![Fr::zero()],
                vec![Fr::zero()],
                vec![Fr::zero()],
                vec![Fr::zero()],
            ]),
        }
    }

    /// Return Option<[Field; 13]> with 4 points (x, y, is_infinite) + partial_address.
    async fn get_public_keys_and_partial_address(
        &self,
        args: &[Vec<Fr>],
    ) -> Result<Vec<Vec<Fr>>, Error> {
        let address = AztecAddress(
            *args
                .first()
                .and_then(|v| v.first())
                .ok_or_else(|| Error::InvalidData("missing address arg".into()))?,
        );

        let Some(complete) = self.address_store.get(&address).await? else {
            tracing::debug!(
                queried_address = %address,
                "getPublicKeysAndPartialAddress: address not found in store"
            );
            return Ok(vec![vec![Fr::zero()], vec![Fr::zero(); 13]]);
        };

        let pk = &complete.public_keys;
        let mut fields = Vec::with_capacity(13);
        for point in [
            &pk.master_nullifier_public_key,
            &pk.master_incoming_viewing_public_key,
            &pk.master_outgoing_viewing_public_key,
            &pk.master_tagging_public_key,
        ] {
            fields.push(point.x);
            fields.push(point.y);
            fields.push(Fr::from(point.is_infinite));
        }
        fields.push(complete.partial_address);
        Ok(vec![vec![Fr::from(true)], fields])
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

        // Collect note hash values that were nullified (from counter map)
        // for pending note filtering.
        let mut nullified_hash_counts: std::collections::HashMap<Fr, usize> =
            std::collections::HashMap::new();
        for nh_counter in self.note_hash_nullifier_counter_map.keys() {
            if let Some(nh) = self
                .note_hashes
                .iter()
                .find(|h| h.note_hash.counter == *nh_counter)
            {
                *nullified_hash_counts.entry(nh.note_hash.value).or_insert(0) += 1;
            }
        }

        // Filter out DB notes that have been consumed during this execution.
        // We check the note's siloed_nullifier against consumed_db_nullifiers
        // (which tracks siloed nullifiers computed from notify_nullified_note).
        notes.retain(|n| {
            if n.siloed_nullifier.is_zero() {
                return true;
            }
            !self.consumed_db_nullifiers.contains(&n.siloed_nullifier)
        });

        // Include pending notes created during this execution that match the
        // query filters (mirrors upstream `noteCache.getNotes(...)` merge).
        // For notes with the same hash, only skip as many as were nullified.
        let mut consumed_hash_counts: std::collections::HashMap<Fr, usize> =
            std::collections::HashMap::new();
        for pending in &self.new_notes {
            if pending.contract_address != self.contract_address {
                continue;
            }
            if pending.storage_slot != storage_slot {
                continue;
            }
            if let Some(owner_addr) = owner {
                if pending.owner != owner_addr {
                    continue;
                }
            }
            // Check the note hasn't been nullified: skip up to the number
            // of times this hash was nullified.
            if let Some(&max_nullified) = nullified_hash_counts.get(&pending.note_hash) {
                let already_consumed = consumed_hash_counts.entry(pending.note_hash).or_insert(0);
                if *already_consumed < max_nullified {
                    *already_consumed += 1;
                    continue;
                }
            }
            notes.push(StoredNote {
                contract_address: pending.contract_address,
                owner: pending.owner,
                storage_slot: pending.storage_slot,
                randomness: pending.randomness,
                note_nonce: Fr::zero(), // nonce unknown during private execution
                note_hash: pending.note_hash,
                siloed_nullifier: Fr::zero(),
                note_data: pending.note_items.clone(),
                nullified: false,
                is_pending: true,
                nullification_block_number: None,
                leaf_index: None,
                block_number: None,
                tx_index_in_block: None,
                note_index_in_tx: None,
                scopes: vec![pending.owner],
            });
        }

        // Apply select-clause filtering (comparators).
        let selects = super::pick_notes::parse_select_clauses(args);
        notes = super::pick_notes::select_notes(notes, &selects);

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
        // Check the persistent store first.
        let mut exists = self
            .note_store
            .has_note(&self.contract_address, note_hash)
            .await?;
        // Also check pending note hashes from the current execution
        // (notes created by sibling calls within the same TX).
        if !exists {
            exists = self
                .note_hashes
                .iter()
                .any(|nh| nh.note_hash.value == *note_hash);
        }
        Ok(vec![vec![Fr::from(exists)]])
    }

    fn notify_created_note(&mut self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        self.ensure_mutable_context()?;
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
        self.ensure_mutable_context()?;
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

        // Track the siloed nullifier so DB notes can be filtered in get_notes.
        let siloed = aztec_core::hash::silo_nullifier(&self.contract_address, &inner_nullifier);
        self.consumed_db_nullifiers.insert(siloed);

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
        self.ensure_mutable_context()?;
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
        fn slot_with_offset(start_slot: Fr, offset: usize) -> Fr {
            let mut bytes = start_slot.to_be_bytes();
            let mut carry = offset as u128;
            for byte in bytes.iter_mut().rev() {
                if carry == 0 {
                    break;
                }
                let sum = u128::from(*byte) + (carry & 0xff);
                *byte = (sum & 0xff) as u8;
                carry = (carry >> 8) + (sum >> 8);
            }
            Fr::from(bytes)
        }

        let (block_hash, contract, start_slot, number_of_elements) = if args.len() >= 4 {
            let block_hash = args
                .first()
                .and_then(|v| v.first())
                .copied()
                .unwrap_or_else(Fr::zero);
            let contract = args
                .get(1)
                .and_then(|v| v.first())
                .ok_or_else(|| Error::InvalidData("missing contract address".into()))?;
            let start_slot = args
                .get(2)
                .and_then(|v| v.first())
                .ok_or_else(|| Error::InvalidData("missing storage slot".into()))?;
            let count = args
                .get(3)
                .and_then(|v| v.first())
                .copied()
                .unwrap_or_else(Fr::zero)
                .to_usize();
            (Some(block_hash), contract, start_slot, count.max(1))
        } else {
            let contract = args
                .first()
                .and_then(|v| v.first())
                .ok_or_else(|| Error::InvalidData("missing contract address".into()))?;
            let slot = args
                .get(1)
                .and_then(|v| v.first())
                .ok_or_else(|| Error::InvalidData("missing storage slot".into()))?;
            (None, contract, slot, 1)
        };

        let contract_addr = AztecAddress(*contract);
        let mut values = Vec::with_capacity(number_of_elements);
        for offset in 0..number_of_elements {
            let slot = slot_with_offset(*start_slot, offset);
            let value = match block_hash.as_ref() {
                Some(block_hash) => {
                    self.node
                        .get_public_storage_at_by_hash(block_hash, &contract_addr, &slot)
                        .await?
                }
                None => {
                    self.node
                        .get_public_storage_at(0, &contract_addr, &slot)
                        .await?
                }
            };
            values.push(value);
        }

        Ok(vec![values])
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
            Some(inst) => Ok(contract_instance_to_fields(&inst.inner)),
            None => Ok(vec![vec![Fr::zero()]; 16]),
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
        self.ensure_mutable_context()?;
        let fields = args.first().cloned().unwrap_or_default();
        let emitted_length = fields.len() as u32;
        self.side_effect_counter += 1;
        let counter = self.side_effect_counter;

        self.private_logs.push(PrivateLogData {
            fields,
            emitted_length,
            note_hash_counter: 0,
            counter,
            contract_address: self.contract_address,
        });
        Ok(vec![])
    }

    fn emit_contract_class_log(&mut self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        self.ensure_mutable_context()?;
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

    /// Look up a value in the execution cache by its hash.
    pub fn get_execution_cache_entry(&self, hash: &Fr) -> Option<Vec<Fr>> {
        self.execution_cache.get(hash).cloned()
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
        // Return a zero-filled witness when not found (Noir expects [Field; 64]).
        Ok(vec![vec![Fr::zero(); 64]])
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
        let note_hash = args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing note hash".into()))?;
        let _witness = self
            .node
            .get_note_hash_membership_witness(0, note_hash)
            .await?;
        // Return the witness as fields (the actual format depends on tree height)
        Ok(vec![])
    }

    async fn get_nullifier_membership_witness(
        &self,
        args: &[Vec<Fr>],
    ) -> Result<Vec<Vec<Fr>>, Error> {
        let nullifier = args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing nullifier".into()))?;
        let _witness = self
            .node
            .get_nullifier_membership_witness(0, nullifier)
            .await?;
        Ok(vec![])
    }

    async fn get_public_data_witness(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        fn fr_at(value: &serde_json::Value, path: &str) -> Result<Fr, Error> {
            let raw = value.pointer(path).ok_or_else(|| {
                Error::InvalidData(format!("public data witness missing field at {path}"))
            })?;
            if let Some(s) = raw.as_str() {
                return parse_field_string(s).map_err(|_| {
                    Error::InvalidData(format!(
                        "public data witness field at {path} has unsupported string value: {s}"
                    ))
                });
            }
            if let Some(n) = raw.as_u64() {
                return Ok(Fr::from(n));
            }
            Err(Error::InvalidData(format!(
                "public data witness field at {path} has unsupported shape: {raw:?}"
            )))
        }

        let block_hash = args
            .first()
            .and_then(|v| v.first())
            .copied()
            .unwrap_or_else(Fr::zero);
        let leaf_slot = args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing leaf slot".into()))?;
        let witness = self
            .node
            .get_public_data_witness_by_hash(&block_hash, leaf_slot)
            .await?;
        let Some(witness) = witness else {
            return Ok(vec![
                vec![Fr::zero()],
                vec![Fr::zero()],
                vec![Fr::zero()],
                vec![Fr::zero()],
                vec![Fr::zero()],
                vec![Fr::zero(); aztec_core::constants::PUBLIC_DATA_TREE_HEIGHT],
            ]);
        };

        let sibling_path = match witness.pointer("/siblingPath") {
            Some(serde_json::Value::Array(entries)) => entries
                .iter()
                .map(|entry| {
                    if let Some(s) = entry.as_str() {
                        parse_field_string(s).map_err(|_| {
                            Error::InvalidData(format!(
                                "public data witness siblingPath entry has unsupported string value: {s}"
                            ))
                        })
                    } else if let Some(n) = entry.as_u64() {
                        Ok(Fr::from(n))
                    } else {
                        Err(Error::InvalidData(format!(
                            "public data witness siblingPath entry has unsupported shape: {entry:?}"
                        )))
                    }
                })
                .collect::<Result<Vec<_>, _>>()?,
            Some(serde_json::Value::String(encoded)) => {
                decode_base64_sibling_path(encoded)?
            }
            _ => {
                return Err(Error::InvalidData(
                    "public data witness missing siblingPath".into(),
                ))
            }
        };

        let mut sibling_path = sibling_path;
        sibling_path.resize(aztec_core::constants::PUBLIC_DATA_TREE_HEIGHT, Fr::zero());
        sibling_path.truncate(aztec_core::constants::PUBLIC_DATA_TREE_HEIGHT);

        Ok(vec![
            vec![fr_at(&witness, "/index")?],
            vec![fr_at(&witness, "/leafPreimage/leaf/slot")?],
            vec![fr_at(&witness, "/leafPreimage/leaf/value")?],
            vec![fr_at(&witness, "/leafPreimage/nextKey")?],
            vec![fr_at(&witness, "/leafPreimage/nextIndex")?],
            sibling_path,
        ])
    }

    async fn get_block_hash_membership_witness(
        &self,
        args: &[Vec<Fr>],
    ) -> Result<Vec<Vec<Fr>>, Error> {
        let block_hash = args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing block hash".into()))?;
        let _witness = self
            .node
            .get_block_hash_membership_witness(0, block_hash)
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

    async fn get_next_app_tag_as_sender(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        use aztec_core::hash::poseidon2_hash;

        let sender = AztecAddress(
            *args
                .first()
                .and_then(|v| v.first())
                .ok_or_else(|| Error::InvalidData("missing sender".into()))?,
        );
        let recipient = AztecAddress(
            *args
                .get(1)
                .and_then(|v| v.first())
                .ok_or_else(|| Error::InvalidData("missing recipient".into()))?,
        );

        // Compute the directional tagging secret (sender → recipient).
        let Some(sender_complete) = self.address_store.get(&sender).await? else {
            return Err(Error::InvalidData(format!(
                "sender {sender} not in address store"
            )));
        };
        let pk_hash = sender_complete.public_keys.hash();
        let ivsk = self
            .key_store
            .get_master_incoming_viewing_secret_key(&pk_hash)
            .await?
            .ok_or_else(|| Error::InvalidData(format!("ivsk not found for sender {sender}")))?;
        let secret = super::utility_oracle::compute_directional_tagging_secret(
            &sender_complete,
            ivsk,
            &recipient,
            &self.contract_address,
            &recipient,
        )?;

        // Get and increment the sender-side tag index.
        let index = self.sender_tagging_store.get_next_index(&secret).await?;

        let tag = poseidon2_hash(&[secret, Fr::from(index)]);
        Ok(vec![vec![tag]])
    }

    async fn check_nullifier_exists(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let inner_nullifier = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing nullifier".into()))?;
        // The Noir oracle passes the inner nullifier. The tree stores siloed
        // nullifiers, so mirror the upstream PXE and silo before lookup.
        let nullifier = aztec_core::hash::silo_nullifier(&self.contract_address, inner_nullifier);
        // Check pending nullifiers first
        if self
            .nullifiers
            .iter()
            .any(|n| n.nullifier.value == nullifier)
        {
            return Ok(vec![vec![Fr::from(true)]]);
        }
        // Check on-chain nullifier tree
        let witness = self
            .node
            .get_nullifier_membership_witness(0, &nullifier)
            .await?;
        let exists = witness.is_some();
        Ok(vec![vec![Fr::from(exists)]])
    }

    /// Execute a nested private function call.
    ///
    /// Mirrors upstream TS `privateCallPrivateFunction`: creates a nested oracle
    /// sharing the same stores, recursively executes the target function via
    /// `AcvmExecutor::execute_private`, then merges side effects back.
    ///
    /// Input args: `[contractAddress], [functionSelector], [argsHash], [sideEffectCounter], [isStaticCall]`
    /// Returns: `[[endSideEffectCounter, returnsHash]]`
    async fn call_private_function(&mut self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let target_address = AztecAddress(
            *args
                .first()
                .and_then(|v| v.first())
                .ok_or_else(|| Error::InvalidData("missing target address".into()))?,
        );
        let selector_field = *args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing function selector".into()))?;
        let args_hash = *args
            .get(2)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing args hash".into()))?;
        let circuit_side_effect_counter = args
            .get(3)
            .and_then(|v| v.first())
            .map(|f| f.to_usize() as u32)
            .unwrap_or(self.side_effect_counter);
        let is_static = args
            .get(4)
            .and_then(|v| v.first())
            .map(|f| *f != Fr::zero())
            .unwrap_or(false);

        // Find the function selector and retrieve cached args up-front so
        // protocol contracts can be handled without requiring local artifacts.
        let selector = aztec_core::abi::FunctionSelector::from_field(selector_field);
        let cached_args = self
            .execution_cache
            .get(&args_hash)
            .cloned()
            .unwrap_or_default();

        if let Some(result) = self.try_handle_protocol_nested_private_call(
            target_address,
            selector,
            &cached_args,
            circuit_side_effect_counter,
            is_static,
        )? {
            return Ok(result);
        }

        // Look up the target contract's artifact.
        let instance = self
            .contract_store
            .get_instance(&target_address)
            .await?
            .ok_or_else(|| {
                Error::InvalidData(format!("nested call: contract not found: {target_address}"))
            })?;
        let artifact = self
            .contract_store
            .get_artifact(&instance.inner.current_contract_class_id)
            .await?
            .ok_or_else(|| {
                Error::InvalidData(format!(
                    "nested call: artifact not found for contract {target_address}"
                ))
            })?;

        // Find the function by selector.
        let function = artifact
            .find_function_by_selector(&selector)
            .ok_or_else(|| {
                Error::InvalidData(format!(
                    "nested call: function with selector {selector} not found in {target_address}"
                ))
            })?;
        let function_name = function.name.clone();

        if function.is_static && !is_static {
            return Err(Error::InvalidData("can only be called statically".into()));
        }

        // Retrieve arguments from the execution cache using the args hash.
        // Build the initial witness: PrivateContextInputs + user args.
        // The context inputs include call_context, block header, tx_context, etc.
        // For nested calls, msg_sender is the calling contract's address.
        let context_inputs_size = artifact.private_context_inputs_size(&function_name);

        // Build the private context inputs witness for the nested call.
        // Reuse the parent's context witness prefix (block header + tx_context)
        // so that chain_id, version, and other context values are correct.
        let mut full_witness = if !self.context_witness_prefix.is_empty()
            && self.context_witness_prefix.len() + 4 <= context_inputs_size
        {
            // Layout: [call_context(4), block_header+tx_context..., side_effect_counter]
            let mut w = Vec::with_capacity(context_inputs_size);
            // Call context
            w.push(self.contract_address.0); // msg_sender = calling contract
            w.push(target_address.0); // contract_address = target
            w.push(selector_field); // function_selector
            w.push(Fr::from(is_static)); // is_static_call
                                         // Block header + tx_context from parent
            w.extend_from_slice(&self.context_witness_prefix);
            // Side effect counter — use the circuit-provided counter so
            // the nested circuit's PrivateContext starts with the correct
            // global counter (the oracle's own counter may have diverged).
            w.push(Fr::from(circuit_side_effect_counter as u64));
            // Pad to context_inputs_size
            w.resize(context_inputs_size, Fr::zero());
            w
        } else {
            let mut w = vec![Fr::zero(); context_inputs_size];
            if w.len() >= 4 {
                w[0] = self.contract_address.0;
                w[1] = target_address.0;
                w[2] = selector_field;
                w[3] = Fr::from(is_static);
            }
            w
        };

        // Append user arguments.
        full_witness.extend_from_slice(&cached_args);

        // Create a nested oracle sharing the same stores.
        let mut nested_oracle = PrivateExecutionOracle::new(
            self.node,
            self.contract_store,
            self.key_store,
            self.note_store,
            self.capsule_store,
            self.address_store,
            self.sender_tagging_store,
            self.block_header.clone(),
            target_address,
            self.protocol_nullifier,
            self.sender_for_tags,
            self.scopes.clone(),
            is_static,
        );

        // Share the execution cache so return values are accessible.
        nested_oracle.execution_cache = self.execution_cache.clone();
        // Share auth witnesses.
        nested_oracle.auth_witnesses = self.auth_witnesses.clone();
        // Start the nested counter from the circuit-provided counter so
        // it stays in sync with the Noir PrivateContext's counter.
        nested_oracle.side_effect_counter = circuit_side_effect_counter;
        // Inherit revertibility threshold so nested calls answer
        // `isSideEffectCounterRevertible` consistently with the parent.
        nested_oracle.min_revertible_side_effect_counter = self.min_revertible_side_effect_counter;
        // Share context witness prefix (block header + tx_context) for nested calls.
        nested_oracle.context_witness_prefix = self.context_witness_prefix.clone();
        // Share parent state so nested calls can see notes/hashes from sibling
        // calls. Track inherited sizes to avoid duplicating during merge.
        nested_oracle.new_notes = self.new_notes.clone();
        nested_oracle.note_hashes = self.note_hashes.clone();
        nested_oracle.nullifiers = self.nullifiers.clone();
        nested_oracle.note_hash_nullifier_counter_map =
            self.note_hash_nullifier_counter_map.clone();
        nested_oracle.consumed_db_nullifiers = self.consumed_db_nullifiers.clone();
        let inherited_new_notes = self.new_notes.len();
        let inherited_note_hashes = self.note_hashes.len();
        let inherited_nullifiers = self.nullifiers.len();
        let inherited_counter_map_keys: std::collections::HashSet<u32> = self
            .note_hash_nullifier_counter_map
            .keys()
            .copied()
            .collect();

        // Execute the nested private function.
        let acvm_output = super::acvm_executor::AcvmExecutor::execute_private(
            &artifact,
            &function_name,
            &full_witness,
            &mut nested_oracle,
        )
        .await?;

        // Compute the actual end counter from the maximum counter across all
        // side effects produced by the nested call. The oracle's side_effect_counter
        // may not advance when counters come from circuit args, so we scan
        // note hashes, nullifiers, and logs to find the true maximum.
        let end_counter = {
            let nh_max = nested_oracle
                .note_hashes
                .iter()
                .skip(inherited_note_hashes)
                .map(|nh| nh.note_hash.counter)
                .max()
                .unwrap_or(0);
            let null_max = nested_oracle
                .nullifiers
                .iter()
                .skip(inherited_nullifiers)
                .map(|n| n.nullifier.counter)
                .max()
                .unwrap_or(0);
            let log_max = nested_oracle
                .private_logs
                .iter()
                .map(|l| l.counter)
                .max()
                .unwrap_or(0);
            let oracle_counter = nested_oracle.side_effect_counter;
            nh_max.max(null_max).max(log_max).max(oracle_counter)
        };

        // Extract returns_hash and end_side_effect_counter from the PCPI
        // in the witness, not from ACIR return values. The PCPI starts at
        // offset `nested_params_size` in the witness.
        let nested_ctx_size_for_pcpi = artifact.private_context_inputs_size(&function_name);
        let pcpi_start = nested_ctx_size_for_pcpi + cached_args.len();
        // PCPI layout: call_context(4), args_hash(1), returns_hash(1), ...
        const PCPI_RETURNS_HASH_OFFSET: usize = 5;

        let returns_hash = {
            let idx = acir::native_types::Witness((pcpi_start + PCPI_RETURNS_HASH_OFFSET) as u32);
            acvm_output
                .witness
                .get(&idx)
                .map(|fe| super::field_conversion::fe_to_fr(fe))
                .unwrap_or_else(|| {
                    // Fallback: compute from return values (may be empty → zero hash)
                    aztec_core::hash::compute_var_args_hash(&acvm_output.return_values)
                })
        };

        // Also store the return values from the execution cache stored by
        // the nested circuit itself (via storeInExecutionCache oracle).
        // The circuit stores its return values at returns_hash before it
        // finishes, so they should already be in the nested oracle's cache.
        // We only need to ensure our cache also has them.
        if !self.execution_cache.contains_key(&returns_hash) {
            if let Some(cached) = nested_oracle.execution_cache.get(&returns_hash) {
                self.execution_cache.insert(returns_hash, cached.clone());
            } else {
                // Store ACVM return values as fallback
                self.execution_cache
                    .insert(returns_hash, acvm_output.return_values.clone());
            }
        }

        // Extract circuit-constrained side effects (private logs, note hashes, etc.)
        // from the nested witness. These are NOT emitted through oracle calls.
        let nested_ctx_size = artifact.private_context_inputs_size(&function_name);
        let nested_params_size = nested_ctx_size + cached_args.len();
        let (circuit_note_hashes, _circuit_nullifiers, circuit_logs) =
            Self::extract_side_effects_from_witness(
                &acvm_output.witness,
                nested_params_size,
                target_address,
            );

        // Capture the nested call's return values for extraction by simulate_tx.
        // Only return_values are stored here — side effects (nullifiers, note
        // hashes, etc.) are merged into the parent oracle below and must NOT be
        // duplicated in nested_execution_results or the kernel will reject the
        // tx with "Duplicate nullifier".
        //
        // For private functions with databus returns, the main circuit's return
        // values are the full PCPI structure.  The user's actual return values
        // live in the first ACIR sub-circuit call (the inner function body),
        // captured by `first_acir_call_return_values`.
        {
            let mut minimal = PrivateCallExecutionResult::default();
            minimal.contract_address = target_address;
            minimal.return_values = if !acvm_output.first_acir_call_return_values.is_empty() {
                acvm_output.first_acir_call_return_values.clone()
            } else {
                acvm_output.return_values.clone()
            };
            self.nested_results.push(minimal);
        }

        // Merge the nested execution cache back into the parent.
        for (k, v) in nested_oracle.execution_cache {
            self.execution_cache.entry(k).or_insert(v);
        }

        // Merge side effects from the nested execution into the parent.
        // Skip inherited items to avoid duplicates — only take new additions.
        let new_note_hashes: Vec<_> = nested_oracle
            .note_hashes
            .into_iter()
            .skip(inherited_note_hashes)
            .collect();
        let oracle_has_note_hashes = !new_note_hashes.is_empty();
        self.note_hashes.extend(new_note_hashes);
        if !oracle_has_note_hashes && !circuit_note_hashes.is_empty() {
            self.note_hashes.extend(circuit_note_hashes);
        }
        self.nullifiers.extend(
            nested_oracle
                .nullifiers
                .into_iter()
                .skip(inherited_nullifiers),
        );
        // Preserve the nested subtree's full log set and replace entries when
        // the witness provides a more accurate version for the same counter.
        self.private_logs.extend(Self::merge_nested_private_logs(
            nested_oracle.private_logs,
            circuit_logs,
        ));
        self.contract_class_logs
            .extend(nested_oracle.contract_class_logs);
        self.new_notes.extend(
            nested_oracle
                .new_notes
                .into_iter()
                .skip(inherited_new_notes),
        );
        self.note_hash_read_requests
            .extend(nested_oracle.note_hash_read_requests);
        self.nullifier_read_requests
            .extend(nested_oracle.nullifier_read_requests);
        self.public_call_requests
            .extend(nested_oracle.public_call_requests);
        self.public_function_calldata
            .extend(nested_oracle.public_function_calldata);
        self.offchain_effects.extend(nested_oracle.offchain_effects);
        for (k, v) in nested_oracle.note_hash_nullifier_counter_map {
            if !inherited_counter_map_keys.contains(&k) {
                self.note_hash_nullifier_counter_map.insert(k, v);
            }
        }
        if nested_oracle.public_teardown_call_request.is_some() {
            self.public_teardown_call_request = nested_oracle.public_teardown_call_request;
        }
        // Merge consumed DB nullifiers from nested call.
        self.consumed_db_nullifiers
            .extend(&nested_oracle.consumed_db_nullifiers);

        // Advance the parent's side effect counter.
        self.side_effect_counter = end_counter;

        // Return [endSideEffectCounter, returnsHash] as a single array.
        Ok(vec![vec![Fr::from(end_counter as u64), returns_hash]])
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
        expiration_timestamp: u64,
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
                is_static_call: self.call_is_static,
            },
            return_values: acvm_output.return_values,
            new_notes: self.new_notes.clone(),
            note_hash_nullifier_counter_map: self.note_hash_nullifier_counter_map.clone(),
            offchain_effects: self.offchain_effects.clone(),
            pre_tags: Vec::new(),
            nested_execution_results: self.nested_results.clone(),
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

        // The first nullifier is always the protocol nullifier (hash of
        // the tx request). Application nullifiers are separate.
        let first_nullifier = self.protocol_nullifier;

        PrivateExecutionResult {
            entrypoint,
            first_nullifier,
            expiration_timestamp,
            public_function_calldata: self.public_function_calldata.clone(),
        }
    }
}

/// Serialize a [`ContractInstance`] into the flat field layout expected by
/// the Noir `utilityGetContractInstance` / `privateGetContractInstance` oracle.
///
/// Field order must match the Noir `ContractInstance` struct:
///   salt, deployer, contract_class_id, initialization_hash,
///   npk_m (x, y, is_infinite), ivpk_m, ovpk_m, tpk_m
pub(crate) fn contract_instance_to_fields(inst: &ContractInstance) -> Vec<Vec<Fr>> {
    let pk = &inst.public_keys;
    vec![
        vec![inst.salt],
        vec![Fr::from(inst.deployer)],
        vec![inst.current_contract_class_id],
        vec![inst.initialization_hash],
        vec![pk.master_nullifier_public_key.x],
        vec![pk.master_nullifier_public_key.y],
        vec![Fr::from(pk.master_nullifier_public_key.is_infinite)],
        vec![pk.master_incoming_viewing_public_key.x],
        vec![pk.master_incoming_viewing_public_key.y],
        vec![Fr::from(pk.master_incoming_viewing_public_key.is_infinite)],
        vec![pk.master_outgoing_viewing_public_key.x],
        vec![pk.master_outgoing_viewing_public_key.y],
        vec![Fr::from(pk.master_outgoing_viewing_public_key.is_infinite)],
        vec![pk.master_tagging_public_key.x],
        vec![pk.master_tagging_public_key.y],
        vec![Fr::from(pk.master_tagging_public_key.is_infinite)],
    ]
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
