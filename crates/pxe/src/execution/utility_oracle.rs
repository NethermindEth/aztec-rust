//! Oracle for utility (view/unconstrained) function execution.

use aztec_core::constants::domain_separator;
use aztec_core::error::Error;
use aztec_core::fee::GasFees;
use aztec_core::grumpkin;
use aztec_core::hash::poseidon2_hash;
use aztec_core::kernel_types::{
    AppendOnlyTreeSnapshot, BlockHeader, GlobalVariables, PartialStateReference, StateReference,
};
use aztec_core::tx::TxHash;
use aztec_core::types::{AztecAddress, Fq, Fr};
use aztec_node_client::AztecNode;

use super::acvm_executor::OracleCallback;
use crate::stores::note_store::{NoteFilter, NoteStatus, StoredNote};
use crate::stores::{
    AddressStore, AnchorBlockStore, CapsuleStore, ContractStore, KeyStore, NoteStore,
    PrivateEventStore, RecipientTaggingStore, SenderStore, SenderTaggingStore,
};
use crate::sync::event_service::EventService;
use crate::sync::log_service::{LogRetrievalRequest, LogService};
use crate::sync::note_service::NoteService;

/// Oracle for utility function execution (read-only, no side effects).
pub struct UtilityExecutionOracle<'a, N: AztecNode> {
    node: &'a N,
    contract_store: &'a ContractStore,
    key_store: &'a KeyStore,
    note_store: &'a NoteStore,
    address_store: &'a AddressStore,
    capsule_store: &'a CapsuleStore,
    sender_store: &'a SenderStore,
    sender_tagging_store: &'a SenderTaggingStore,
    recipient_tagging_store: &'a RecipientTaggingStore,
    private_event_store: &'a PrivateEventStore,
    anchor_block_store: &'a AnchorBlockStore,
    block_header: serde_json::Value,
    contract_address: AztecAddress,
    scopes: Vec<AztecAddress>,
    /// Auth witnesses available for this execution.
    auth_witnesses: Vec<(Fr, Vec<Fr>)>,
}

impl<'a, N: AztecNode> UtilityExecutionOracle<'a, N> {
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
        address_store: &'a AddressStore,
        capsule_store: &'a CapsuleStore,
        sender_store: &'a SenderStore,
        sender_tagging_store: &'a SenderTaggingStore,
        recipient_tagging_store: &'a RecipientTaggingStore,
        private_event_store: &'a PrivateEventStore,
        anchor_block_store: &'a AnchorBlockStore,
        block_header: serde_json::Value,
        contract_address: AztecAddress,
        scopes: Vec<AztecAddress>,
    ) -> Self {
        Self {
            node,
            contract_store,
            key_store,
            note_store,
            address_store,
            capsule_store,
            sender_store,
            sender_tagging_store,
            recipient_tagging_store,
            private_event_store,
            anchor_block_store,
            block_header,
            contract_address,
            scopes,
            auth_witnesses: Vec::new(),
        }
    }

    /// Set auth witnesses for this execution context.
    pub fn set_auth_witnesses(&mut self, witnesses: Vec<(Fr, Vec<Fr>)>) {
        self.auth_witnesses = witnesses;
    }

    /// Handle an ACVM foreign call for a utility function.
    ///
    /// Supports both prefixed names (from compiled Noir bytecode) and
    /// legacy unprefixed names.
    pub async fn handle_foreign_call(
        &self,
        name: &str,
        args: Vec<Vec<Fr>>,
    ) -> Result<Vec<Vec<Fr>>, Error> {
        // Strip the common prefixes used by compiled Noir bytecode
        let stripped = name
            .strip_prefix("utility")
            .or_else(|| name.strip_prefix("private"))
            .unwrap_or(name);

        let handler = if !stripped.is_empty() {
            let mut chars = stripped.chars();
            let first = chars.next().unwrap().to_lowercase().to_string();
            format!("{first}{}", chars.as_str())
        } else {
            name.to_owned()
        };

        match handler.as_str() {
            // Storage
            "getPublicStorageAt" | "storageRead" => self.get_public_storage_at(&args).await,
            "getContractInstance" => self.get_contract_instance(&args).await,

            // Notes
            "getNotes" => self.get_notes(&args).await,
            "checkNullifierExists" => self.check_nullifier_exists(&args).await,

            // Keys
            "getPublicKeysAndPartialAddress" | "tryGetPublicKeysAndPartialAddress" => {
                self.get_public_keys_and_partial_address(&args).await
            }
            "getKeyValidationRequest" | "getSecretKey" => {
                self.get_key_validation_request(&args).await
            }

            // Block header
            "getBlockHeader" => self.get_block_header(&args),
            "getUtilityContext" => self.get_utility_context(),

            // Auth witnesses
            "getAuthWitness" => self.get_auth_witness(&args),

            // Membership witnesses
            "getNoteHashMembershipWitness" => Ok(vec![vec![]]),
            "getNullifierMembershipWitness" => Ok(vec![vec![]]),
            "getLowNullifierMembershipWitness" => {
                self.get_low_nullifier_membership_witness(&args).await
            }
            "getBlockHashMembershipWitness" => Ok(vec![vec![]]),
            "getPublicDataWitness" => Ok(vec![vec![]]),
            "getL1ToL2MembershipWitness" => Ok(vec![vec![]]),

            // Misc
            "getRandomField" => Ok(vec![vec![Fr::random()]]),
            "assertCompatibleOracleVersion" => Ok(vec![]),
            "log" => Ok(vec![]),
            "aes128Decrypt" => self.aes128_decrypt(&args),
            "getSharedSecret" => self.get_shared_secret(&args).await,

            // Capsules
            "loadCapsule" | "getCapsule" => self.load_capsule(&args).await,
            "storeCapsule" => self.store_capsule(&args).await,
            "deleteCapsule" => self.delete_capsule(&args).await,
            "copyCapsule" => self.copy_capsule(&args).await,

            // Tagging and log discovery
            "fetchTaggedLogs" => self.fetch_tagged_logs(&args).await,
            "bulkRetrieveLogs" => self.bulk_retrieve_logs(&args).await,
            "validateAndStoreEnqueuedNotesAndEvents" => {
                self.validate_and_store_enqueued_notes_and_events(&args)
                    .await
            }
            "emitOffchainEffect" => Ok(vec![]),

            _ => {
                tracing::warn!(
                    oracle = name,
                    handler = handler.as_str(),
                    "unknown utility oracle call"
                );
                Ok(vec![])
            }
        }
    }

    fn fr_at(val: &serde_json::Value, path: &str) -> Fr {
        match val.pointer(path) {
            Some(serde_json::Value::String(s)) => Fr::from_hex(s).unwrap_or(Fr::zero()),
            Some(serde_json::Value::Number(n)) => Fr::from(n.as_u64().unwrap_or(0)),
            _ => Fr::zero(),
        }
    }

    fn u64_at(val: &serde_json::Value, path: &str) -> u64 {
        match val.pointer(path) {
            Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0),
            Some(serde_json::Value::String(s)) => {
                if let Some(hex) = s.strip_prefix("0x") {
                    u64::from_str_radix(hex, 16).unwrap_or(0)
                } else {
                    s.parse::<u64>().unwrap_or(0)
                }
            }
            _ => 0,
        }
    }

    fn u128_at(val: &serde_json::Value, path: &str) -> u128 {
        match val.pointer(path) {
            Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0) as u128,
            Some(serde_json::Value::String(s)) => {
                if let Some(hex) = s.strip_prefix("0x") {
                    u128::from_str_radix(hex, 16).unwrap_or(0)
                } else {
                    s.parse::<u128>().unwrap_or(0)
                }
            }
            _ => 0,
        }
    }

    fn eth_at(val: &serde_json::Value, path: &str) -> aztec_core::types::EthAddress {
        match val.pointer(path).and_then(|v| v.as_str()) {
            Some(s) => {
                let fr = Fr::from_hex(s).unwrap_or(Fr::zero());
                let bytes = fr.to_be_bytes();
                let mut addr = [0u8; 20];
                addr.copy_from_slice(&bytes[12..32]);
                aztec_core::types::EthAddress(addr)
            }
            None => aztec_core::types::EthAddress::default(),
        }
    }

    fn snapshot_at(val: &serde_json::Value, prefix: &str) -> AppendOnlyTreeSnapshot {
        AppendOnlyTreeSnapshot {
            root: Self::fr_at(val, &format!("{prefix}/root")),
            next_available_leaf_index: Self::u64_at(
                val,
                &format!("{prefix}/nextAvailableLeafIndex"),
            ) as u32,
        }
    }

    fn parse_block_header(&self) -> BlockHeader {
        let h = &self.block_header;
        BlockHeader {
            last_archive: Self::snapshot_at(h, "/lastArchive"),
            state: StateReference {
                l1_to_l2_message_tree: Self::snapshot_at(h, "/state/l1ToL2MessageTree"),
                partial: PartialStateReference {
                    note_hash_tree: Self::snapshot_at(h, "/state/partial/noteHashTree"),
                    nullifier_tree: Self::snapshot_at(h, "/state/partial/nullifierTree"),
                    public_data_tree: Self::snapshot_at(h, "/state/partial/publicDataTree"),
                },
            },
            sponge_blob_hash: Self::fr_at(h, "/spongeBlobHash"),
            global_variables: GlobalVariables {
                chain_id: Self::fr_at(h, "/globalVariables/chainId"),
                version: Self::fr_at(h, "/globalVariables/version"),
                block_number: Self::u64_at(h, "/globalVariables/blockNumber"),
                slot_number: Self::u64_at(h, "/globalVariables/slotNumber"),
                timestamp: Self::u64_at(h, "/globalVariables/timestamp"),
                coinbase: Self::eth_at(h, "/globalVariables/coinbase"),
                fee_recipient: AztecAddress(Self::fr_at(h, "/globalVariables/feeRecipient")),
                gas_fees: GasFees {
                    fee_per_da_gas: Self::u128_at(h, "/globalVariables/gasFees/feePerDaGas"),
                    fee_per_l2_gas: Self::u128_at(h, "/globalVariables/gasFees/feePerL2Gas"),
                },
            },
            total_fees: Self::fr_at(h, "/totalFees"),
            total_mana_used: Self::fr_at(h, "/totalManaUsed"),
        }
    }

    fn get_utility_context(&self) -> Result<Vec<Vec<Fr>>, Error> {
        let mut outputs: Vec<Vec<Fr>> = self
            .parse_block_header()
            .to_fields()
            .into_iter()
            .map(|f| vec![f])
            .collect();
        outputs.push(vec![self.contract_address.0]);
        Ok(outputs)
    }

    fn get_block_header(&self, _args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        Ok(self
            .parse_block_header()
            .to_fields()
            .into_iter()
            .map(|f| vec![f])
            .collect())
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

        let inst = self.contract_store.get_instance(&addr).await?;
        let inst = match inst {
            Some(i) => Some(i),
            None => self.node.get_contract(&addr).await?,
        };

        match inst {
            Some(inst) => Ok(super::oracle::contract_instance_to_fields(&inst.inner)),
            None => Ok(vec![vec![Fr::zero()]; 16]),
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
                scopes: self.scopes.clone(),
                ..Default::default()
            })
            .await?;

        // Apply select-clause filtering (comparators).
        let selects = super::pick_notes::parse_select_clauses(args);
        notes = super::pick_notes::select_notes(notes, &selects);

        tracing::trace!(
            contract = %self.contract_address,
            ?owner,
            slot = %storage_slot,
            scopes = self.scopes.len(),
            found = notes.len(),
            "utility_get_notes"
        );
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

    async fn store_capsule(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let contract_address =
            AztecAddress(*args.first().and_then(|v| v.first()).ok_or_else(|| {
                Error::InvalidData("storeCapsule: missing contract address".into())
            })?);
        let slot = *args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("storeCapsule: missing slot".into()))?;
        let capsule = args.get(2).cloned().unwrap_or_default();

        self.ensure_contract_db_access(&contract_address)?;
        self.capsule_store
            .store_capsule(&contract_address, &slot, &capsule)
            .await?;
        Ok(vec![])
    }

    async fn load_capsule(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let contract_address =
            AztecAddress(*args.first().and_then(|v| v.first()).ok_or_else(|| {
                Error::InvalidData("loadCapsule: missing contract address".into())
            })?);
        let slot = *args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("loadCapsule: missing slot".into()))?;
        let array_len = args
            .get(2)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or_else(Fr::zero)
            .to_usize();

        self.ensure_contract_db_access(&contract_address)?;
        let maybe_values = self
            .capsule_store
            .load_capsule(&contract_address, &slot)
            .await?;
        let is_some = maybe_values.is_some();
        let mut values = maybe_values.unwrap_or_default();
        values.resize(array_len, Fr::zero());
        Ok(vec![vec![Fr::from(is_some)], values])
    }

    async fn delete_capsule(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let contract_address =
            AztecAddress(*args.first().and_then(|v| v.first()).ok_or_else(|| {
                Error::InvalidData("deleteCapsule: missing contract address".into())
            })?);
        let slot = *args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("deleteCapsule: missing slot".into()))?;

        self.ensure_contract_db_access(&contract_address)?;
        self.capsule_store
            .delete_capsule(&contract_address, &slot)
            .await?;
        Ok(vec![])
    }

    async fn copy_capsule(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let contract_address =
            AztecAddress(*args.first().and_then(|v| v.first()).ok_or_else(|| {
                Error::InvalidData("copyCapsule: missing contract address".into())
            })?);
        let src_slot = *args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("copyCapsule: missing src slot".into()))?;
        let dst_slot = *args
            .get(2)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("copyCapsule: missing dst slot".into()))?;
        let num_entries = args
            .get(3)
            .and_then(|v| v.first())
            .copied()
            .unwrap_or_else(Fr::zero)
            .to_usize();

        self.ensure_contract_db_access(&contract_address)?;
        self.capsule_store
            .copy_capsule(&contract_address, &src_slot, &dst_slot, num_entries)
            .await?;
        Ok(vec![])
    }

    async fn fetch_tagged_logs(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let pending_tagged_log_array_base_slot =
            *args.first().and_then(|v| v.first()).ok_or_else(|| {
                Error::InvalidData("fetchTaggedLogs: missing capsule array base slot".into())
            })?;

        if self.scopes.is_empty() {
            return Ok(vec![]);
        }

        let log_service = LogService::new(
            self.node,
            self.sender_store,
            self.sender_tagging_store,
            self.recipient_tagging_store,
            self.capsule_store,
        );

        for scope in &self.scopes {
            let secrets = self.tagging_secrets_for_recipient(scope).await?;
            if secrets.is_empty() {
                continue;
            }
            let logs = log_service
                .fetch_tagged_logs(&self.contract_address, scope, &secrets)
                .await?;
            if logs.is_empty() {
                continue;
            }
            let serialized = logs
                .into_iter()
                .map(|log| serialize_pending_tagged_log(&log, scope))
                .collect::<Result<Vec<_>, _>>()?;
            self.capsule_store
                .append_to_capsule_array(
                    &self.contract_address,
                    &pending_tagged_log_array_base_slot,
                    &serialized,
                )
                .await?;
        }

        Ok(vec![])
    }

    async fn bulk_retrieve_logs(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let contract_address =
            AztecAddress(*args.first().and_then(|v| v.first()).ok_or_else(|| {
                Error::InvalidData("bulkRetrieveLogs: missing contract address".into())
            })?);
        let requests_slot = *args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("bulkRetrieveLogs: missing requests slot".into()))?;
        let responses_slot = *args
            .get(2)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("bulkRetrieveLogs: missing responses slot".into()))?;

        self.ensure_contract_db_access(&contract_address)?;

        let requests = self
            .capsule_store
            .read_capsule_array(&contract_address, &requests_slot)
            .await?
            .into_iter()
            .map(parse_log_retrieval_request)
            .collect::<Result<Vec<_>, _>>()?;

        let log_service = LogService::new(
            self.node,
            self.sender_store,
            self.sender_tagging_store,
            self.recipient_tagging_store,
            self.capsule_store,
        );
        let maybe_responses = log_service.bulk_retrieve_logs(&requests).await?;

        self.capsule_store
            .set_capsule_array(&contract_address, &requests_slot, &[])
            .await?;

        let serialized = maybe_responses
            .into_iter()
            .map(|logs| serialize_log_retrieval_option(logs.first()))
            .collect::<Result<Vec<_>, _>>()?;
        self.capsule_store
            .set_capsule_array(&contract_address, &responses_slot, &serialized)
            .await?;

        Ok(vec![])
    }

    async fn validate_and_store_enqueued_notes_and_events(
        &self,
        args: &[Vec<Fr>],
    ) -> Result<Vec<Vec<Fr>>, Error> {
        let contract_address =
            AztecAddress(*args.first().and_then(|v| v.first()).ok_or_else(|| {
                Error::InvalidData(
                    "validateAndStoreEnqueuedNotesAndEvents: missing contract address".into(),
                )
            })?);
        let note_requests_slot = *args.get(1).and_then(|v| v.first()).ok_or_else(|| {
            Error::InvalidData(
                "validateAndStoreEnqueuedNotesAndEvents: missing note requests slot".into(),
            )
        })?;
        let event_requests_slot = *args.get(2).and_then(|v| v.first()).ok_or_else(|| {
            Error::InvalidData(
                "validateAndStoreEnqueuedNotesAndEvents: missing event requests slot".into(),
            )
        })?;

        self.ensure_contract_db_access(&contract_address)?;

        let note_requests = self
            .capsule_store
            .read_capsule_array(&contract_address, &note_requests_slot)
            .await?;
        let note_service = NoteService::new(self.node, self.note_store);
        for fields in note_requests {
            let request = parse_note_validation_request(&fields)?;
            note_service
                .validate_and_store_note(
                    &crate::stores::note_store::StoredNote {
                        contract_address: request.contract_address,
                        owner: request.owner,
                        storage_slot: request.storage_slot,
                        randomness: request.randomness,
                        note_nonce: request.note_nonce,
                        note_hash: request.note_hash,
                        siloed_nullifier: aztec_core::hash::silo_nullifier(
                            &request.contract_address,
                            &request.nullifier,
                        ),
                        note_data: request.content,
                        nullified: false,
                        is_pending: false,
                        nullification_block_number: None,
                        leaf_index: None,
                        block_number: None,
                        tx_index_in_block: None,
                        note_index_in_tx: None,
                        scopes: vec![request.recipient],
                    },
                    &request.recipient,
                )
                .await?;
        }

        let event_requests = self
            .capsule_store
            .read_capsule_array(&contract_address, &event_requests_slot)
            .await?;
        let event_service =
            EventService::new(self.node, self.private_event_store, self.anchor_block_store);
        for fields in event_requests {
            let request = parse_event_validation_request(&fields)?;
            event_service
                .validate_and_store_event(
                    &request.contract_address,
                    &request.event_type_id,
                    request.randomness,
                    request.serialized_event,
                    request.event_commitment,
                    request.tx_hash,
                    &request.recipient,
                )
                .await?;
        }

        self.capsule_store
            .set_capsule_array(&contract_address, &note_requests_slot, &[])
            .await?;
        self.capsule_store
            .set_capsule_array(&contract_address, &event_requests_slot, &[])
            .await?;

        Ok(vec![])
    }

    async fn check_nullifier_exists(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        let inner_nullifier = args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing nullifier".into()))?;
        // Silo before looking up — the tree stores siloed nullifiers
        let siloed = aztec_core::hash::silo_nullifier(&self.contract_address, inner_nullifier);
        let witness = self
            .node
            .get_nullifier_membership_witness(0, &siloed)
            .await?;
        Ok(vec![vec![Fr::from(witness.is_some())]])
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

    /// Return `NullifierMembershipWitness` from the node.
    ///
    /// The Noir struct has 5 deserialization slots:
    /// - index (1 field)
    /// - leaf_preimage.nullifier (1 field)
    /// - leaf_preimage.next_nullifier (1 field)
    /// - leaf_preimage.next_index (1 field)
    /// - path (NULLIFIER_TREE_HEIGHT = 42 fields)
    /// Parse a JSON value as a field element (hex string) or a number.
    fn parse_field_or_number(val: Option<&serde_json::Value>) -> Fr {
        val.and_then(|v| {
            if let Some(s) = v.as_str() {
                Fr::from_hex(s)
                    .ok()
                    .or_else(|| s.parse::<u64>().ok().map(Fr::from))
            } else {
                v.as_u64().map(Fr::from)
            }
        })
        .unwrap_or(Fr::zero())
    }

    async fn get_low_nullifier_membership_witness(
        &self,
        args: &[Vec<Fr>],
    ) -> Result<Vec<Vec<Fr>>, Error> {
        let _block_hash = args
            .first()
            .and_then(|v| v.first())
            .copied()
            .unwrap_or(Fr::zero());
        let nullifier = args.get(1).and_then(|v| v.first()).ok_or_else(|| {
            Error::InvalidData("getLowNullifierMembershipWitness: missing nullifier".into())
        })?;

        let witness_json = self
            .node
            .get_low_nullifier_membership_witness(0, nullifier)
            .await?;

        // Parse the JSON response from the node into the 5-slot format
        // expected by the Noir `NullifierMembershipWitness` struct.
        //
        // Node response format:
        // { "index": "N", "leafPreimage": { "leaf": { "nullifier": "0x..." },
        //   "nextKey": "0x...", "nextIndex": "N" }, "siblingPath": "<base64>" }
        if let Some(json) = witness_json {
            let index = Self::parse_field_or_number(json.get("index"));

            let preimage = json.get("leafPreimage").unwrap_or(&json);
            let leaf = preimage.get("leaf").unwrap_or(preimage);
            let nullifier_val = leaf
                .get("nullifier")
                .and_then(|v| v.as_str())
                .and_then(|s| Fr::from_hex(s).ok())
                .unwrap_or(Fr::zero());
            let next_nullifier = preimage
                .get("nextKey")
                .or_else(|| preimage.get("nextNullifier"))
                .and_then(|v| v.as_str())
                .and_then(|s| Fr::from_hex(s).ok())
                .unwrap_or(Fr::zero());
            let next_index = Self::parse_field_or_number(preimage.get("nextIndex"));

            // siblingPath is base64-encoded binary: 42 x 32-byte BE field elements
            let path = json
                .get("siblingPath")
                .and_then(|v| v.as_str())
                .and_then(|s| {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD.decode(s).ok()
                })
                .map(|bytes| {
                    bytes
                        .chunks(32)
                        .map(|chunk| {
                            let mut padded = [0u8; 32];
                            let start = 32usize.saturating_sub(chunk.len());
                            padded[start..].copy_from_slice(chunk);
                            Fr::from(padded)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| vec![Fr::zero(); 42]);

            let mut path = path;
            path.resize(42, Fr::zero());

            Ok(vec![
                vec![index],
                vec![nullifier_val],
                vec![next_nullifier],
                vec![next_index],
                path,
            ])
        } else {
            // Return zeros if witness not found
            Ok(vec![
                vec![Fr::zero()],
                vec![Fr::zero()],
                vec![Fr::zero()],
                vec![Fr::zero()],
                vec![Fr::zero(); 42],
            ])
        }
    }

    /// Return `KeyValidationRequest { pk_m: Point, sk_app: Field }` (4 fields).
    ///
    /// Enforces scope isolation: only keys belonging to accounts in the
    /// current execution scopes are accessible.
    async fn get_key_validation_request(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        use aztec_core::hash::poseidon2_hash;

        let pk_m_hash = *args
            .first()
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("missing pk_m_hash".into()))?;

        // Check scope: ensure the key owner is within the current scopes
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

    /// Compute shared secret: `address_secret * ephPk`.
    async fn get_shared_secret(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        use aztec_core::grumpkin;

        let recipient = AztecAddress(
            *args
                .first()
                .and_then(|v| v.first())
                .ok_or_else(|| Error::InvalidData("getSharedSecret: missing recipient".into()))?,
        );
        let eph_pk_x = *args
            .get(1)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("getSharedSecret: missing eph_pk.x".into()))?;
        let eph_pk_y = *args
            .get(2)
            .and_then(|v| v.first())
            .ok_or_else(|| Error::InvalidData("getSharedSecret: missing eph_pk.y".into()))?;
        let eph_pk_is_infinite = args
            .get(3)
            .and_then(|v| v.first())
            .map(|f| f.to_usize() != 0)
            .unwrap_or(false);

        if eph_pk_is_infinite {
            return Ok(vec![
                vec![Fr::zero()],
                vec![Fr::zero()],
                vec![Fr::from(true)],
            ]);
        }

        let Some(complete) = self.address_store.get(&recipient).await? else {
            return Err(Error::InvalidData(format!(
                "getSharedSecret: recipient {recipient} not in address store"
            )));
        };
        let pk_hash = complete.public_keys.hash();
        let Some(ivsk) = self
            .key_store
            .get_master_incoming_viewing_secret_key(&pk_hash)
            .await?
        else {
            return Err(Error::InvalidData(format!(
                "getSharedSecret: ivsk not found for {recipient}"
            )));
        };

        let preaddress = aztec_core::hash::poseidon2_hash_with_separator(
            &[pk_hash, complete.partial_address],
            aztec_core::constants::domain_separator::CONTRACT_ADDRESS_V1,
        );
        let address_secret = compute_address_secret(preaddress, ivsk);

        let eph_pk = aztec_core::types::Point {
            x: eph_pk_x,
            y: eph_pk_y,
            is_infinite: false,
        };
        let shared = grumpkin::scalar_mul(&address_secret, &eph_pk);

        Ok(vec![
            vec![shared.x],
            vec![shared.y],
            vec![Fr::from(shared.is_infinite)],
        ])
    }

    /// AES-128 CBC decryption oracle.
    fn aes128_decrypt(&self, args: &[Vec<Fr>]) -> Result<Vec<Vec<Fr>>, Error> {
        use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
        type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

        let ct_storage = args.first().cloned().unwrap_or_default();
        let max_len = ct_storage.len();
        let ct_len = args
            .get(1)
            .and_then(|v| v.first())
            .map(|f| f.to_usize())
            .unwrap_or(0);
        let iv_fields = args.get(2).cloned().unwrap_or_default();
        let key_fields = args.get(3).cloned().unwrap_or_default();

        let ciphertext: Vec<u8> = ct_storage
            .iter()
            .take(ct_len)
            .map(|f| f.to_usize() as u8)
            .collect();
        let iv: [u8; 16] = iv_fields
            .iter()
            .take(16)
            .map(|f| f.to_usize() as u8)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap_or([0u8; 16]);
        let key: [u8; 16] = key_fields
            .iter()
            .take(16)
            .map(|f| f.to_usize() as u8)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap_or([0u8; 16]);

        let plaintext = match Aes128CbcDec::new(&key.into(), &iv.into())
            .decrypt_padded_vec_mut::<Pkcs7>(&ciphertext)
        {
            Ok(pt) => pt,
            Err(e) => {
                return Err(Error::InvalidData(format!("aes128 decrypt error: {e}")));
            }
        };

        let pt_len = plaintext.len();
        let mut storage: Vec<Fr> = plaintext.iter().map(|&b| Fr::from(u64::from(b))).collect();
        storage.resize(max_len, Fr::zero());
        Ok(vec![storage, vec![Fr::from(pt_len as u64)]])
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
        Ok(vec![vec![]])
    }

    fn ensure_contract_db_access(&self, contract_address: &AztecAddress) -> Result<(), Error> {
        if *contract_address != self.contract_address {
            return Err(Error::InvalidData(format!(
                "contract {} is not allowed to access {}'s PXE DB",
                contract_address, self.contract_address
            )));
        }
        Ok(())
    }

    async fn tagging_secrets_for_recipient(
        &self,
        recipient: &AztecAddress,
    ) -> Result<Vec<Fr>, Error> {
        let Some(complete_address) = self.address_store.get(recipient).await? else {
            return Ok(vec![]);
        };

        let pk_hash = complete_address.public_keys.hash();
        let Some(ivsk) = self
            .key_store
            .get_master_incoming_viewing_secret_key(&pk_hash)
            .await?
        else {
            return Ok(vec![]);
        };

        let mut senders = self.sender_store.get_all().await?;
        // Also include all addresses in the address store as potential
        // senders — registered accounts are skipped by register_sender()
        // but can still be senders for tag computation.
        for addr in self.address_store.get_all().await? {
            if !senders.contains(&addr.address) {
                senders.push(addr.address);
            }
        }
        if !senders.contains(recipient) {
            senders.push(*recipient);
        }

        let mut secrets = Vec::with_capacity(senders.len());
        for sender in senders {
            secrets.push(compute_directional_tagging_secret(
                &complete_address,
                ivsk,
                &sender,
                &self.contract_address,
                recipient,
            )?);
        }
        Ok(secrets)
    }
}

const MAX_NOTE_PACKED_LEN: usize = 8;
const MAX_EVENT_SERIALIZED_LEN: usize = 10;
const MAX_NOTE_HASHES_PER_TX: usize = 64;
const PRIVATE_LOG_SIZE_IN_FIELDS: usize = aztec_core::constants::PRIVATE_LOG_SIZE_IN_FIELDS;
const PRIVATE_LOG_CIPHERTEXT_LEN: usize = 15;

#[derive(Debug)]
struct ParsedNoteValidationRequest {
    contract_address: AztecAddress,
    owner: AztecAddress,
    storage_slot: Fr,
    randomness: Fr,
    note_nonce: Fr,
    content: Vec<Fr>,
    note_hash: Fr,
    nullifier: Fr,
    #[allow(dead_code)]
    tx_hash: TxHash,
    recipient: AztecAddress,
}

#[derive(Debug)]
struct ParsedEventValidationRequest {
    contract_address: AztecAddress,
    event_type_id: aztec_core::abi::EventSelector,
    randomness: Fr,
    serialized_event: Vec<Fr>,
    event_commitment: Fr,
    tx_hash: TxHash,
    recipient: AztecAddress,
}

fn parse_log_retrieval_request(fields: Vec<Fr>) -> Result<LogRetrievalRequest, Error> {
    if fields.len() < 2 {
        return Err(Error::InvalidData("log retrieval request too short".into()));
    }
    Ok(LogRetrievalRequest {
        is_public: true,
        contract_address: Some(AztecAddress(fields[0])),
        tag: fields[1],
    })
}

fn serialize_bounded_vec(values: &[Fr], max_length: usize) -> Result<Vec<Fr>, Error> {
    if values.len() > max_length {
        return Err(Error::InvalidData(format!(
            "bounded vec overflow: {} > {}",
            values.len(),
            max_length
        )));
    }
    let mut storage = values.to_vec();
    storage.resize(max_length, Fr::zero());
    storage.push(Fr::from(values.len() as u64));
    Ok(storage)
}

fn serialize_log_retrieval_option(
    log: Option<&crate::sync::log_service::TaggedLog>,
) -> Result<Vec<Fr>, Error> {
    let mut out = Vec::new();
    match log {
        Some(log) => {
            out.push(Fr::from(true));
            let payload = if log.data.is_empty() {
                &[][..]
            } else {
                &log.data[1..]
            };
            out.extend(serialize_bounded_vec(
                payload,
                MAX_NOTE_PACKED_LEN.max(PRIVATE_LOG_CIPHERTEXT_LEN),
            )?);
            out.push(tx_hash_to_field(&log.tx_hash)?);
            out.extend(serialize_bounded_vec(
                &log.note_hashes,
                MAX_NOTE_HASHES_PER_TX,
            )?);
            out.push(log.first_nullifier);
        }
        None => {
            out.push(Fr::zero());
            out.extend(vec![
                Fr::zero();
                MAX_NOTE_PACKED_LEN.max(PRIVATE_LOG_CIPHERTEXT_LEN) + 1
            ]);
            out.push(Fr::zero());
            out.extend(vec![Fr::zero(); MAX_NOTE_HASHES_PER_TX + 1]);
            out.push(Fr::zero());
        }
    }
    Ok(out)
}

fn serialize_pending_tagged_log(
    log: &crate::sync::log_service::TaggedLog,
    recipient: &AztecAddress,
) -> Result<Vec<Fr>, Error> {
    let mut out = serialize_bounded_vec(&log.data, PRIVATE_LOG_SIZE_IN_FIELDS)?;
    out.push(tx_hash_to_field(&log.tx_hash)?);
    out.extend(serialize_bounded_vec(
        &log.note_hashes,
        MAX_NOTE_HASHES_PER_TX,
    )?);
    out.push(log.first_nullifier);
    out.push(recipient.0);
    Ok(out)
}

fn parse_note_validation_request(fields: &[Fr]) -> Result<ParsedNoteValidationRequest, Error> {
    if fields.len() < 5 + MAX_NOTE_PACKED_LEN + 5 {
        return Err(Error::InvalidData(
            "note validation request too short".into(),
        ));
    }
    let contract_address = AztecAddress(fields[0]);
    let owner = AztecAddress(fields[1]);
    let storage_slot = fields[2];
    let randomness = fields[3];
    let note_nonce = fields[4];
    let content_len = fields[5 + MAX_NOTE_PACKED_LEN]
        .to_usize()
        .min(MAX_NOTE_PACKED_LEN);
    let content = fields[5..5 + MAX_NOTE_PACKED_LEN][..content_len].to_vec();
    let note_hash = fields[5 + MAX_NOTE_PACKED_LEN + 1];
    let nullifier = fields[5 + MAX_NOTE_PACKED_LEN + 2];
    let tx_hash = tx_hash_from_field(fields[5 + MAX_NOTE_PACKED_LEN + 3]);
    let recipient = AztecAddress(fields[5 + MAX_NOTE_PACKED_LEN + 4]);
    Ok(ParsedNoteValidationRequest {
        contract_address,
        owner,
        storage_slot,
        randomness,
        note_nonce,
        content,
        note_hash,
        nullifier,
        tx_hash,
        recipient,
    })
}

fn parse_event_validation_request(fields: &[Fr]) -> Result<ParsedEventValidationRequest, Error> {
    if fields.len() < 3 + MAX_EVENT_SERIALIZED_LEN + 4 {
        return Err(Error::InvalidData(
            "event validation request too short".into(),
        ));
    }
    let contract_address = AztecAddress(fields[0]);
    let event_type_id = aztec_core::abi::EventSelector(fields[1]);
    let randomness = fields[2];
    let event_len = fields[3 + MAX_EVENT_SERIALIZED_LEN]
        .to_usize()
        .min(MAX_EVENT_SERIALIZED_LEN);
    let serialized_event = fields[3..3 + MAX_EVENT_SERIALIZED_LEN][..event_len].to_vec();
    let event_commitment = fields[3 + MAX_EVENT_SERIALIZED_LEN + 1];
    let tx_hash = tx_hash_from_field(fields[3 + MAX_EVENT_SERIALIZED_LEN + 2]);
    let recipient = AztecAddress(fields[3 + MAX_EVENT_SERIALIZED_LEN + 3]);
    Ok(ParsedEventValidationRequest {
        contract_address,
        event_type_id,
        randomness,
        serialized_event,
        event_commitment,
        tx_hash,
        recipient,
    })
}

fn tx_hash_from_field(field: Fr) -> TxHash {
    TxHash(field.to_be_bytes())
}

fn tx_hash_to_field(tx_hash: &TxHash) -> Result<Fr, Error> {
    Fr::from_hex(&tx_hash.to_string())
}

pub(crate) fn compute_directional_tagging_secret(
    local_address: &aztec_core::types::CompleteAddress,
    local_ivsk: Fq,
    external_address: &AztecAddress,
    app: &AztecAddress,
    recipient: &AztecAddress,
) -> Result<Fr, Error> {
    let public_keys_hash = local_address.public_keys.hash();
    let preaddress = aztec_core::hash::poseidon2_hash_with_separator(
        &[public_keys_hash, local_address.partial_address],
        domain_separator::CONTRACT_ADDRESS_V1,
    );
    let address_secret = compute_address_secret(preaddress, local_ivsk);
    let external_point = grumpkin::point_from_x(external_address.0)?;
    let shared_secret = grumpkin::scalar_mul(&address_secret, &external_point);
    let app_tagging_secret = poseidon2_hash(&[shared_secret.x, shared_secret.y, app.0]);
    Ok(poseidon2_hash(&[app_tagging_secret, recipient.0]))
}

fn compute_address_secret(preaddress: Fr, ivsk: Fq) -> Fq {
    let candidate = Fq(ivsk.0 + Fq::from_be_bytes_mod_order(&preaddress.to_be_bytes()).0);
    let address_point_candidate = grumpkin::scalar_mul(&candidate, &grumpkin::generator());
    if grumpkin::has_positive_y(&address_point_candidate) {
        candidate
    } else {
        Fq(-candidate.0)
    }
}

#[async_trait::async_trait]
impl<'a, N: AztecNode + Send + Sync + 'static> OracleCallback for UtilityExecutionOracle<'a, N> {
    async fn handle_foreign_call(
        &mut self,
        function: &str,
        inputs: Vec<Vec<Fr>>,
    ) -> Result<Vec<Vec<Fr>>, Error> {
        UtilityExecutionOracle::handle_foreign_call(self, function, inputs).await
    }
}
