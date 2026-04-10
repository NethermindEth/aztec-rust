//! Simulated kernel proving — generates kernel public inputs in software.
//!
//! Ports the TS `generateSimulatedProvingResult` from
//! `contract_function_simulator.ts` which processes a `PrivateExecutionResult`
//! through side-effect squashing, siloing, gas metering, and splitting into
//! revertible/non-revertible accumulated data without running real kernel circuits.

use aztec_core::constants::*;
use aztec_core::error::Error;
use aztec_core::fee::Gas;
use aztec_core::hash::{
    compute_note_hash_nonce, compute_siloed_private_log_first_field, compute_unique_note_hash,
    poseidon2_hash, silo_note_hash, silo_nullifier,
};
use aztec_core::kernel_types::{
    pad_fields, PartialPrivateTailPublicInputsForPublic, PartialPrivateTailPublicInputsForRollup,
    PrivateKernelTailPublicInputs, PrivateLog, PrivateToPublicAccumulatedData,
    PrivateToRollupAccumulatedData, PublicCallRequest, ScopedL2ToL1Message, ScopedLogHash,
    ScopedNoteHash, ScopedNullifier, TxConstantData,
};
use aztec_core::types::Fr;

use crate::execution::execution_result::{PrivateExecutionResult, PrivateLogData};

/// Output from simulated kernel processing.
#[derive(Debug, Clone)]
pub struct SimulatedKernelOutput {
    /// The typed private kernel tail public inputs.
    pub public_inputs: PrivateKernelTailPublicInputs,
}

/// Assembles kernel public inputs in software (no proving).
///
/// This is the Rust port of TS `generateSimulatedProvingResult`.
pub struct SimulatedKernel;

impl SimulatedKernel {
    /// Process a private execution result into simulated kernel output.
    ///
    /// Steps (matching upstream):
    /// 1. Collect all side effects from the execution tree
    /// 2. Squash transient note hash / nullifier pairs
    /// 3. Verify read requests (skipped in simulation — node does this)
    /// 4. Silo note hashes, nullifiers, and private log first fields
    /// 5. For private-only txs: compute unique note hashes
    /// 6. Split into revertible / non-revertible
    /// 7. Meter gas usage
    /// 8. Build the PrivateKernelTailPublicInputs
    pub fn process(
        execution_result: &PrivateExecutionResult,
        constants: TxConstantData,
        fee_payer: &Fr,
        expiration_timestamp: u64,
    ) -> Result<SimulatedKernelOutput, Error> {
        // Step 1: Collect all side effects from the execution tree
        let all_note_hashes: Vec<ScopedNoteHash> = execution_result
            .all_note_hashes()
            .into_iter()
            .cloned()
            .collect();
        let all_nullifiers: Vec<ScopedNullifier> = execution_result
            .all_nullifiers()
            .into_iter()
            .cloned()
            .collect();
        let all_private_logs: Vec<PrivateLogData> = execution_result
            .all_private_logs()
            .into_iter()
            .cloned()
            .collect();
        let mut all_public_call_requests: Vec<_> = execution_result
            .all_public_call_requests()
            .into_iter()
            .cloned()
            .collect();
        let all_contract_class_logs = execution_result.all_contract_class_logs_sorted();
        let note_hash_nullifier_counter_map =
            execution_result.all_note_hash_nullifier_counter_maps();

        let is_private_only = all_public_call_requests.is_empty()
            && execution_result.get_teardown_call_request().is_none();

        let min_revertible_counter = execution_result
            .entrypoint
            .min_revertible_side_effect_counter;

        // Step 2: Squash transient pairs
        let (mut filtered_note_hashes, mut filtered_nullifiers, mut filtered_private_logs) =
            squash_transient_side_effects(
                &all_note_hashes,
                &all_nullifiers,
                &all_private_logs,
                &note_hash_nullifier_counter_map,
                min_revertible_counter,
            );

        filtered_note_hashes.sort_by_key(|nh| nh.note_hash.counter);
        filtered_nullifiers.sort_by_key(|n| n.nullifier.counter);
        filtered_private_logs.sort_by_key(|log| log.counter);
        all_public_call_requests.sort_by_key(|req| req.counter);

        // Step 3: Silo note hashes, nullifiers, and private logs
        let siloed_note_hashes: Vec<Fr> = filtered_note_hashes
            .iter()
            .map(|nh| {
                if nh.is_empty() {
                    Fr::zero()
                } else {
                    silo_note_hash(&nh.contract_address, &nh.note_hash.value)
                }
            })
            .collect();

        let siloed_nullifiers: Vec<Fr> = filtered_nullifiers
            .iter()
            .map(|n| {
                if n.is_empty() {
                    Fr::zero()
                } else {
                    silo_nullifier(&n.contract_address, &n.nullifier.value)
                }
            })
            .collect();

        let siloed_private_logs: Vec<PrivateLog> = filtered_private_logs
            .iter()
            .map(|log| {
                let mut fields = log.fields.clone();
                if !fields.is_empty() && fields[0] != Fr::zero() {
                    fields[0] =
                        compute_siloed_private_log_first_field(&log.contract_address, &fields[0]);
                }
                PrivateLog {
                    fields: pad_fields(fields, PRIVATE_LOG_SIZE_IN_FIELDS),
                    emitted_length: log.emitted_length,
                }
            })
            .collect();

        // Collect contract class log hashes
        let contract_class_log_hashes: Vec<ScopedLogHash> = all_contract_class_logs
            .iter()
            .map(|ccl| ScopedLogHash {
                log_hash: aztec_core::kernel_types::LogHash {
                    value: poseidon2_hash(
                        &aztec_core::tx::ContractClassLogFields::from_emitted_fields(
                            ccl.log.fields.clone(),
                        )
                        .fields,
                    ),
                    length: ccl.log.emitted_length,
                },
                contract_address: ccl.log.contract_address,
            })
            .collect();

        // Collect L2-to-L1 messages (not yet implemented in execution)
        let l2_to_l1_msgs: Vec<ScopedL2ToL1Message> = Vec::new();

        if is_private_only {
            // Step 4 (private-only): Compute unique note hashes
            let first_nullifier = execution_result.first_nullifier;
            // The protocol nullifier must always be at position 0.
            let mut rollup_nullifiers = vec![first_nullifier];
            rollup_nullifiers.extend(siloed_nullifiers.iter().copied());
            let unique_note_hashes: Vec<Fr> = siloed_note_hashes
                .iter()
                .enumerate()
                .map(|(i, siloed_hash)| {
                    if *siloed_hash == Fr::zero() {
                        Fr::zero()
                    } else {
                        let nonce = compute_note_hash_nonce(&first_nullifier, i);
                        compute_unique_note_hash(&nonce, siloed_hash)
                    }
                })
                .collect();

            // Step 5: Build for-rollup accumulated data
            let end = PrivateToRollupAccumulatedData {
                note_hashes: pad_fields(unique_note_hashes, MAX_NOTE_HASHES_PER_TX),
                nullifiers: pad_fields(rollup_nullifiers, MAX_NULLIFIERS_PER_TX),
                l2_to_l1_msgs: pad_to_scoped_l2_to_l1(l2_to_l1_msgs),
                private_logs: pad_private_logs(siloed_private_logs),
                contract_class_logs_hashes: pad_scoped_log_hashes(contract_class_log_hashes),
            };

            // Step 6: Meter gas
            let gas_used = meter_gas_rollup(&end);

            Ok(SimulatedKernelOutput {
                public_inputs: PrivateKernelTailPublicInputs {
                    constants,
                    gas_used,
                    fee_payer: aztec_core::types::AztecAddress(*fee_payer),
                    expiration_timestamp,
                    for_public: None,
                    for_rollup: Some(PartialPrivateTailPublicInputsForRollup { end }),
                },
            })
        } else {
            // Step 4 (public): Split into revertible / non-revertible
            let (nr_note_hashes, r_note_hashes) = split_note_hashes_by_counter(
                &siloed_note_hashes,
                &filtered_note_hashes,
                min_revertible_counter,
            );
            let (mut nr_nullifiers, r_nullifiers) = split_nullifiers_by_counter(
                &siloed_nullifiers,
                &filtered_nullifiers,
                min_revertible_counter,
            );
            let (nr_private_logs, r_private_logs) = split_private_logs(
                &siloed_private_logs,
                &filtered_private_logs,
                min_revertible_counter,
            );

            // The protocol nullifier (derived from tx request hash) must always
            // be at position 0 of the non-revertible nullifiers. The sequencer
            // and nonce computation depend on this being the first nullifier.
            nr_nullifiers.insert(0, execution_result.first_nullifier);

            // Uniquify ALL note hashes, matching TS generateSimulatedProvingResult.
            // The nonce generator is the first nullifier (protocol nullifier).
            let nonce_generator = execution_result.first_nullifier;
            let nr_unique_note_hashes: Vec<Fr> = nr_note_hashes
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    if *h == Fr::zero() {
                        Fr::zero()
                    } else {
                        let nonce = compute_note_hash_nonce(&nonce_generator, i);
                        compute_unique_note_hash(&nonce, h)
                    }
                })
                .collect();
            // Revertible note hashes are NOT uniquified for public txs —
            // the sequencer/public kernel handles uniquification.

            // Build public call request arrays
            let mut nr_public_calls = Vec::new();
            let mut r_public_calls = Vec::new();
            for req in &all_public_call_requests {
                let pcr = PublicCallRequest {
                    msg_sender: req.msg_sender,
                    contract_address: req.contract_address,
                    is_static_call: req.is_static_call,
                    calldata_hash: req.calldata_hash,
                };
                if req.counter < min_revertible_counter {
                    nr_public_calls.push(pcr);
                } else {
                    r_public_calls.push(pcr);
                }
            }

            let teardown = execution_result
                .get_teardown_call_request()
                .map(|req| PublicCallRequest {
                    msg_sender: req.msg_sender,
                    contract_address: req.contract_address,
                    is_static_call: req.is_static_call,
                    calldata_hash: req.calldata_hash,
                })
                .unwrap_or_default();

            let non_revertible = PrivateToPublicAccumulatedData {
                note_hashes: pad_fields(nr_unique_note_hashes, MAX_NOTE_HASHES_PER_TX),
                nullifiers: pad_fields(nr_nullifiers, MAX_NULLIFIERS_PER_TX),
                l2_to_l1_msgs: pad_to_scoped_l2_to_l1(Vec::new()),
                private_logs: pad_private_logs(nr_private_logs),
                contract_class_logs_hashes: pad_scoped_log_hashes(
                    contract_class_log_hashes.clone(),
                ),
                public_call_requests: pad_public_call_requests(nr_public_calls),
            };

            let revertible = PrivateToPublicAccumulatedData {
                note_hashes: pad_fields(r_note_hashes, MAX_NOTE_HASHES_PER_TX),
                nullifiers: pad_fields(r_nullifiers, MAX_NULLIFIERS_PER_TX),
                l2_to_l1_msgs: pad_to_scoped_l2_to_l1(Vec::new()),
                private_logs: pad_private_logs(r_private_logs),
                contract_class_logs_hashes: pad_scoped_log_hashes(Vec::new()),
                public_call_requests: pad_public_call_requests(r_public_calls),
            };

            // Meter gas (use public overhead)
            let gas_used = meter_gas_public(&non_revertible, &revertible);

            Ok(SimulatedKernelOutput {
                public_inputs: PrivateKernelTailPublicInputs {
                    constants,
                    gas_used,
                    fee_payer: aztec_core::types::AztecAddress(*fee_payer),
                    expiration_timestamp,
                    for_public: Some(PartialPrivateTailPublicInputsForPublic {
                        non_revertible_accumulated_data: non_revertible,
                        revertible_accumulated_data: revertible,
                        public_teardown_call_request: teardown,
                    }),
                    for_rollup: None,
                },
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Squashing
// ---------------------------------------------------------------------------

/// Remove transient note-hash/nullifier pairs that cancel each other out.
fn squash_transient_side_effects(
    note_hashes: &[ScopedNoteHash],
    nullifiers: &[ScopedNullifier],
    private_logs: &[PrivateLogData],
    note_hash_nullifier_counter_map: &std::collections::HashMap<u32, u32>,
    _min_revertible_counter: u32,
) -> (
    Vec<ScopedNoteHash>,
    Vec<ScopedNullifier>,
    Vec<PrivateLogData>,
) {
    // Build set of squashed note hash counters
    let mut squashed_note_hash_counters = std::collections::HashSet::new();
    let mut squashed_nullifier_counters = std::collections::HashSet::new();

    for (nh_counter, null_counter) in note_hash_nullifier_counter_map {
        // Both the note hash and its nullifier are transient — squash both
        squashed_note_hash_counters.insert(*nh_counter);
        squashed_nullifier_counters.insert(*null_counter);
    }

    let filtered_note_hashes: Vec<ScopedNoteHash> = note_hashes
        .iter()
        .filter(|nh| !squashed_note_hash_counters.contains(&nh.note_hash.counter))
        .cloned()
        .collect();

    let filtered_nullifiers: Vec<ScopedNullifier> = nullifiers
        .iter()
        .filter(|n| !squashed_nullifier_counters.contains(&n.nullifier.counter))
        .cloned()
        .collect();

    // Filter private logs whose note hash was squashed
    let filtered_logs: Vec<PrivateLogData> = private_logs
        .iter()
        .filter(|log| !squashed_note_hash_counters.contains(&log.counter))
        .cloned()
        .collect();

    (filtered_note_hashes, filtered_nullifiers, filtered_logs)
}

// ---------------------------------------------------------------------------
// Splitting by revertibility
// ---------------------------------------------------------------------------

/// Split siloed note hash fields by the revertibility counter.
fn split_note_hashes_by_counter(
    siloed: &[Fr],
    originals: &[ScopedNoteHash],
    min_revertible: u32,
) -> (Vec<Fr>, Vec<Fr>) {
    let mut non_revertible = Vec::new();
    let mut revertible = Vec::new();
    for (i, s) in siloed.iter().enumerate() {
        if let Some(orig) = originals.get(i) {
            if orig.note_hash.counter < min_revertible {
                non_revertible.push(*s);
            } else {
                revertible.push(*s);
            }
        }
    }
    (non_revertible, revertible)
}

/// Split siloed nullifier fields by the revertibility counter.
fn split_nullifiers_by_counter(
    siloed: &[Fr],
    originals: &[ScopedNullifier],
    min_revertible: u32,
) -> (Vec<Fr>, Vec<Fr>) {
    let mut non_revertible = Vec::new();
    let mut revertible = Vec::new();
    for (i, s) in siloed.iter().enumerate() {
        if let Some(orig) = originals.get(i) {
            if orig.nullifier.counter < min_revertible {
                non_revertible.push(*s);
            } else {
                revertible.push(*s);
            }
        }
    }
    (non_revertible, revertible)
}

fn split_private_logs(
    siloed: &[PrivateLog],
    originals: &[PrivateLogData],
    min_revertible: u32,
) -> (Vec<PrivateLog>, Vec<PrivateLog>) {
    let mut non_revertible = Vec::new();
    let mut revertible = Vec::new();
    for (i, s) in siloed.iter().enumerate() {
        if let Some(orig) = originals.get(i) {
            if orig.counter < min_revertible {
                non_revertible.push(s.clone());
            } else {
                revertible.push(s.clone());
            }
        }
    }
    (non_revertible, revertible)
}

// ---------------------------------------------------------------------------
// Gas metering
// ---------------------------------------------------------------------------

fn meter_gas_rollup(data: &PrivateToRollupAccumulatedData) -> Gas {
    let note_hash_count = data
        .note_hashes
        .iter()
        .filter(|h| **h != Fr::zero())
        .count() as u64;
    let nullifier_count = data.nullifiers.iter().filter(|h| **h != Fr::zero()).count() as u64;
    let l2_to_l1_count = data.l2_to_l1_msgs.iter().filter(|m| !m.is_empty()).count() as u64;
    let log_count = data.private_logs.iter().filter(|l| !l.is_empty()).count() as u64;
    let class_log_count = data
        .contract_class_logs_hashes
        .iter()
        .filter(|h| !h.is_empty())
        .count() as u64;

    let l2_gas = PRIVATE_TX_L2_GAS_OVERHEAD
        + note_hash_count * L2_GAS_PER_NOTE_HASH
        + nullifier_count * L2_GAS_PER_NULLIFIER
        + l2_to_l1_count * L2_GAS_PER_L2_TO_L1_MSG
        + log_count * L2_GAS_PER_PRIVATE_LOG
        + class_log_count * L2_GAS_PER_CONTRACT_CLASS_LOG;

    let da_fields =
        note_hash_count + nullifier_count + (log_count * PRIVATE_LOG_SIZE_IN_FIELDS as u64);
    let da_gas = TX_DA_GAS_OVERHEAD + da_fields * DA_GAS_PER_FIELD;

    Gas::new(da_gas, l2_gas)
}

fn meter_gas_public(
    non_revertible: &PrivateToPublicAccumulatedData,
    revertible: &PrivateToPublicAccumulatedData,
) -> Gas {
    let count_non_empty_fields =
        |fields: &[Fr]| -> u64 { fields.iter().filter(|h| **h != Fr::zero()).count() as u64 };

    let note_hashes = count_non_empty_fields(&non_revertible.note_hashes)
        + count_non_empty_fields(&revertible.note_hashes);
    let nullifiers = count_non_empty_fields(&non_revertible.nullifiers)
        + count_non_empty_fields(&revertible.nullifiers);
    let logs = (non_revertible
        .private_logs
        .iter()
        .filter(|l| !l.is_empty())
        .count()
        + revertible
            .private_logs
            .iter()
            .filter(|l| !l.is_empty())
            .count()) as u64;

    let l2_gas = PUBLIC_TX_L2_GAS_OVERHEAD
        + note_hashes * L2_GAS_PER_NOTE_HASH
        + nullifiers * L2_GAS_PER_NULLIFIER
        + logs * L2_GAS_PER_PRIVATE_LOG;

    let da_fields = note_hashes + nullifiers + (logs * PRIVATE_LOG_SIZE_IN_FIELDS as u64);
    let da_gas = TX_DA_GAS_OVERHEAD + da_fields * DA_GAS_PER_FIELD;

    Gas::new(da_gas, l2_gas)
}

// ---------------------------------------------------------------------------
// Padding helpers
// ---------------------------------------------------------------------------

fn pad_to_scoped_l2_to_l1(mut v: Vec<ScopedL2ToL1Message>) -> Vec<ScopedL2ToL1Message> {
    while v.len() < MAX_L2_TO_L1_MSGS_PER_TX {
        v.push(ScopedL2ToL1Message::empty());
    }
    v
}

fn pad_private_logs(mut v: Vec<PrivateLog>) -> Vec<PrivateLog> {
    while v.len() < MAX_PRIVATE_LOGS_PER_TX {
        v.push(PrivateLog::empty());
    }
    v
}

fn pad_scoped_log_hashes(mut v: Vec<ScopedLogHash>) -> Vec<ScopedLogHash> {
    while v.len() < MAX_CONTRACT_CLASS_LOGS_PER_TX {
        v.push(ScopedLogHash::empty());
    }
    v
}

fn pad_public_call_requests(mut v: Vec<PublicCallRequest>) -> Vec<PublicCallRequest> {
    while v.len() < MAX_ENQUEUED_CALLS_PER_TX {
        v.push(PublicCallRequest::empty());
    }
    v
}
