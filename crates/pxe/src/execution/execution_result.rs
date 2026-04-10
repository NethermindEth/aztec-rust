//! Typed private execution results matching upstream stdlib.
//!
//! These types replace the simplified `PrivateExecutionResult` that only carried
//! flat return values. The real types preserve the full execution tree structure
//! needed by the kernel prover and `TxProvingResult.toTx()`.

use acir::native_types::WitnessMap;
use acir::FieldElement;

use aztec_core::kernel_types::{
    CallContext, CountedContractClassLog, NoteAndSlot, ScopedNoteHash, ScopedNullifier,
    ScopedReadRequest,
};
use aztec_core::tx::HashedValues;
use aztec_core::types::{AztecAddress, Fr};

/// Result of a single private call execution (one function in the call tree).
///
/// Matches TS `PrivateCallExecutionResult`.
#[derive(Debug, Clone)]
pub struct PrivateCallExecutionResult {
    /// ACIR bytecode (gzipped) for the circuit.
    pub acir: Vec<u8>,
    /// Verification key (raw bytes).
    pub vk: Vec<u8>,
    /// Partial witness map — the solved ACVM witness.
    pub partial_witness: WitnessMap<FieldElement>,
    /// The contract address that was called.
    pub contract_address: AztecAddress,
    /// The call context for this execution.
    pub call_context: CallContext,
    /// Function return values.
    pub return_values: Vec<Fr>,

    // --- Side effects collected by the oracle ---
    /// Notes created during this call.
    pub new_notes: Vec<NoteAndSlot>,
    /// Maps note hash counter -> nullifier counter (for transient squashing).
    pub note_hash_nullifier_counter_map: std::collections::HashMap<u32, u32>,
    /// Offchain effects emitted via oracle.
    pub offchain_effects: Vec<Vec<Fr>>,
    /// Pre-tags used for private log encryption.
    pub pre_tags: Vec<Fr>,
    /// Nested private call results (recursive tree structure).
    pub nested_execution_results: Vec<PrivateCallExecutionResult>,
    /// Contract class logs emitted during this call.
    pub contract_class_logs: Vec<CountedContractClassLog>,

    // --- Scoped side effects (counter-bearing, for kernel input) ---
    /// Note hashes with counters and contract scope.
    pub note_hashes: Vec<ScopedNoteHash>,
    /// Nullifiers with counters and contract scope.
    pub nullifiers: Vec<ScopedNullifier>,
    /// Note hash read requests from this call.
    pub note_hash_read_requests: Vec<ScopedReadRequest>,
    /// Nullifier read requests from this call.
    pub nullifier_read_requests: Vec<ScopedReadRequest>,
    /// Encrypted log data emitted.
    pub private_logs: Vec<PrivateLogData>,
    /// Public function call requests enqueued.
    pub public_call_requests: Vec<PublicCallRequestData>,
    /// Teardown call request (at most one per tx).
    pub public_teardown_call_request: Option<PublicCallRequestData>,

    /// The side-effect counter at the start of this call.
    pub start_side_effect_counter: u32,
    /// The side-effect counter at the end of this call.
    pub end_side_effect_counter: u32,
    /// Minimum revertible side effect counter (set by the entrypoint).
    pub min_revertible_side_effect_counter: u32,
}

impl Default for PrivateCallExecutionResult {
    fn default() -> Self {
        Self {
            acir: Vec::new(),
            vk: Vec::new(),
            partial_witness: WitnessMap::default(),
            contract_address: AztecAddress::zero(),
            call_context: CallContext::default(),
            return_values: Vec::new(),
            new_notes: Vec::new(),
            note_hash_nullifier_counter_map: std::collections::HashMap::new(),
            offchain_effects: Vec::new(),
            pre_tags: Vec::new(),
            nested_execution_results: Vec::new(),
            contract_class_logs: Vec::new(),
            note_hashes: Vec::new(),
            nullifiers: Vec::new(),
            note_hash_read_requests: Vec::new(),
            nullifier_read_requests: Vec::new(),
            private_logs: Vec::new(),
            public_call_requests: Vec::new(),
            public_teardown_call_request: None,
            start_side_effect_counter: 0,
            end_side_effect_counter: 0,
            min_revertible_side_effect_counter: 0,
        }
    }
}

/// Top-level result from private execution.
///
/// Matches TS `PrivateExecutionResult`.
#[derive(Debug, Clone)]
pub struct PrivateExecutionResult {
    /// The entrypoint call execution result (root of the call tree).
    pub entrypoint: PrivateCallExecutionResult,
    /// The first nullifier (protocol nullifier / nonce generator).
    pub first_nullifier: Fr,
    /// Calldata preimages for enqueued public calls.
    pub public_function_calldata: Vec<HashedValues>,
}

impl PrivateExecutionResult {
    /// Iterate all call results in the execution tree (depth-first).
    pub fn iter_all_calls(&self) -> Vec<&PrivateCallExecutionResult> {
        let mut results = Vec::new();
        Self::collect_calls(&self.entrypoint, &mut results);
        results
    }

    fn collect_calls<'a>(
        call: &'a PrivateCallExecutionResult,
        out: &mut Vec<&'a PrivateCallExecutionResult>,
    ) {
        out.push(call);
        for nested in &call.nested_execution_results {
            Self::collect_calls(nested, out);
        }
    }

    /// Collect all note hashes from the execution tree.
    pub fn all_note_hashes(&self) -> Vec<&ScopedNoteHash> {
        self.iter_all_calls()
            .into_iter()
            .flat_map(|c| c.note_hashes.iter())
            .collect()
    }

    /// Collect all nullifiers from the execution tree.
    pub fn all_nullifiers(&self) -> Vec<&ScopedNullifier> {
        self.iter_all_calls()
            .into_iter()
            .flat_map(|c| c.nullifiers.iter())
            .collect()
    }

    /// Collect all note hash read requests.
    pub fn all_note_hash_read_requests(&self) -> Vec<&ScopedReadRequest> {
        self.iter_all_calls()
            .into_iter()
            .flat_map(|c| c.note_hash_read_requests.iter())
            .collect()
    }

    /// Collect all nullifier read requests.
    pub fn all_nullifier_read_requests(&self) -> Vec<&ScopedReadRequest> {
        self.iter_all_calls()
            .into_iter()
            .flat_map(|c| c.nullifier_read_requests.iter())
            .collect()
    }

    /// Collect all private logs from the execution tree.
    pub fn all_private_logs(&self) -> Vec<&PrivateLogData> {
        self.iter_all_calls()
            .into_iter()
            .flat_map(|c| c.private_logs.iter())
            .collect()
    }

    /// Collect all contract class logs, sorted by counter.
    pub fn all_contract_class_logs_sorted(&self) -> Vec<&CountedContractClassLog> {
        let mut logs: Vec<&CountedContractClassLog> = self
            .iter_all_calls()
            .into_iter()
            .flat_map(|c| c.contract_class_logs.iter())
            .collect();
        logs.sort_by_key(|l| l.counter);
        logs
    }

    /// Collect all public call requests from the execution tree.
    pub fn all_public_call_requests(&self) -> Vec<&PublicCallRequestData> {
        self.iter_all_calls()
            .into_iter()
            .flat_map(|c| c.public_call_requests.iter())
            .collect()
    }

    /// Get the teardown call request (if any).
    pub fn get_teardown_call_request(&self) -> Option<&PublicCallRequestData> {
        self.iter_all_calls()
            .into_iter()
            .find_map(|c| c.public_teardown_call_request.as_ref())
    }

    /// Collect the note hash -> nullifier counter map from all calls.
    pub fn all_note_hash_nullifier_counter_maps(&self) -> std::collections::HashMap<u32, u32> {
        let mut map = std::collections::HashMap::new();
        for call in self.iter_all_calls() {
            map.extend(&call.note_hash_nullifier_counter_map);
        }
        map
    }
}

/// Private log data with counter for ordering.
#[derive(Debug, Clone)]
pub struct PrivateLogData {
    /// The log fields.
    pub fields: Vec<Fr>,
    /// Emitted length (non-padded).
    pub emitted_length: u32,
    /// Counter of the note hash this log is associated with (for squashing).
    pub note_hash_counter: u32,
    /// Side-effect counter.
    pub counter: u32,
    /// Contract that emitted this log.
    pub contract_address: AztecAddress,
}

/// Data for an enqueued public function call.
#[derive(Debug, Clone)]
pub struct PublicCallRequestData {
    /// Target contract address.
    pub contract_address: AztecAddress,
    /// Caller address.
    pub msg_sender: AztecAddress,
    /// Whether this is a static call.
    pub is_static_call: bool,
    /// Hash of the calldata.
    pub calldata_hash: Fr,
    /// Side-effect counter.
    pub counter: u32,
}
