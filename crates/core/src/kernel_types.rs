//! Typed kernel and transaction data structures matching upstream stdlib.
//!
//! These types carry the exact field ordering and semantics needed by:
//! - `PrivateKernelTailCircuitPublicInputs`
//! - `TxProvingResult.toTx()`
//! - `DataTxValidator` on the node
//!
//! All field orderings match the TS stdlib classes in
//! `yarn-project/stdlib/src/kernel/` and `yarn-project/stdlib/src/tx/`.

use serde::{Deserialize, Serialize};

use crate::constants::*;
use crate::fee::{Gas, GasFees, GasSettings};
use crate::hash::poseidon2_hash_with_separator;
use crate::types::{AztecAddress, EthAddress, Fr};

fn eth_address_to_fr(address: &EthAddress) -> Fr {
    let mut bytes = [0u8; 32];
    bytes[12..].copy_from_slice(&address.0);
    Fr::from(bytes)
}

// ---------------------------------------------------------------------------
// Leaf types
// ---------------------------------------------------------------------------

/// A note hash with a side-effect counter.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteHash {
    pub value: Fr,
    pub counter: u32,
}

impl NoteHash {
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.value == Fr::zero()
    }
}

/// A note hash scoped to a contract address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopedNoteHash {
    pub note_hash: NoteHash,
    pub contract_address: AztecAddress,
}

impl ScopedNoteHash {
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.note_hash.is_empty()
    }
}

/// A nullifier with its linked note hash and side-effect counter.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nullifier {
    pub value: Fr,
    pub note_hash: Fr,
    pub counter: u32,
}

impl Nullifier {
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.value == Fr::zero()
    }
}

/// A nullifier scoped to a contract address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopedNullifier {
    pub nullifier: Nullifier,
    pub contract_address: AztecAddress,
}

impl ScopedNullifier {
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.nullifier.is_empty()
    }
}

/// An L2-to-L1 message.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct L2ToL1Message {
    pub recipient: EthAddress,
    pub content: Fr,
}

/// An L2-to-L1 message scoped to a contract address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopedL2ToL1Message {
    pub message: L2ToL1Message,
    pub contract_address: AztecAddress,
}

impl ScopedL2ToL1Message {
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.message.content == Fr::zero()
    }
}

/// A log hash with its value and emitted length.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogHash {
    pub value: Fr,
    pub length: u32,
}

impl LogHash {
    pub fn is_empty(&self) -> bool {
        self.value == Fr::zero()
    }
}

/// A log hash scoped to a contract address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopedLogHash {
    pub log_hash: LogHash,
    pub contract_address: AztecAddress,
}

impl ScopedLogHash {
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.log_hash.is_empty()
    }
}

/// A private log — fixed-width fields plus emitted length.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateLog {
    pub fields: Vec<Fr>,
    pub emitted_length: u32,
}

impl Default for PrivateLog {
    fn default() -> Self {
        Self {
            fields: vec![Fr::zero(); PRIVATE_LOG_SIZE_IN_FIELDS],
            emitted_length: 0,
        }
    }
}

impl PrivateLog {
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.fields.iter().all(|f| *f == Fr::zero())
    }
}

// ---------------------------------------------------------------------------
// Public call request
// ---------------------------------------------------------------------------

/// A request to execute a public function, as seen by the kernel.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicCallRequest {
    pub msg_sender: AztecAddress,
    pub contract_address: AztecAddress,
    pub is_static_call: bool,
    pub calldata_hash: Fr,
}

impl PublicCallRequest {
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.contract_address == AztecAddress::zero()
    }
}

/// A public call request with a counter (used inside PrivateCircuitPublicInputs).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountedPublicCallRequest {
    pub inner: PublicCallRequest,
    pub counter: u32,
}

// ---------------------------------------------------------------------------
// Accumulated data types
// ---------------------------------------------------------------------------

/// Accumulated data from private execution destined for public execution.
///
/// Field ordering matches TS `PrivateToPublicAccumulatedData`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateToPublicAccumulatedData {
    pub note_hashes: Vec<Fr>,
    pub nullifiers: Vec<Fr>,
    pub l2_to_l1_msgs: Vec<ScopedL2ToL1Message>,
    pub private_logs: Vec<PrivateLog>,
    pub contract_class_logs_hashes: Vec<ScopedLogHash>,
    pub public_call_requests: Vec<PublicCallRequest>,
}

impl PrivateToPublicAccumulatedData {
    pub fn empty() -> Self {
        Self {
            note_hashes: vec![Fr::zero(); MAX_NOTE_HASHES_PER_TX],
            nullifiers: vec![Fr::zero(); MAX_NULLIFIERS_PER_TX],
            l2_to_l1_msgs: vec![ScopedL2ToL1Message::empty(); MAX_L2_TO_L1_MSGS_PER_TX],
            private_logs: vec![PrivateLog::empty(); MAX_PRIVATE_LOGS_PER_TX],
            contract_class_logs_hashes: vec![
                ScopedLogHash::empty();
                MAX_CONTRACT_CLASS_LOGS_PER_TX
            ],
            public_call_requests: vec![PublicCallRequest::empty(); MAX_ENQUEUED_CALLS_PER_TX],
        }
    }
}

/// Accumulated data from private execution destined directly for rollup.
///
/// Field ordering matches TS `PrivateToRollupAccumulatedData`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateToRollupAccumulatedData {
    pub note_hashes: Vec<Fr>,
    pub nullifiers: Vec<Fr>,
    pub l2_to_l1_msgs: Vec<ScopedL2ToL1Message>,
    pub private_logs: Vec<PrivateLog>,
    pub contract_class_logs_hashes: Vec<ScopedLogHash>,
}

impl PrivateToRollupAccumulatedData {
    pub fn empty() -> Self {
        Self {
            note_hashes: vec![Fr::zero(); MAX_NOTE_HASHES_PER_TX],
            nullifiers: vec![Fr::zero(); MAX_NULLIFIERS_PER_TX],
            l2_to_l1_msgs: vec![ScopedL2ToL1Message::empty(); MAX_L2_TO_L1_MSGS_PER_TX],
            private_logs: vec![PrivateLog::empty(); MAX_PRIVATE_LOGS_PER_TX],
            contract_class_logs_hashes: vec![
                ScopedLogHash::empty();
                MAX_CONTRACT_CLASS_LOGS_PER_TX
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Tree snapshots and state
// ---------------------------------------------------------------------------

/// A Merkle tree snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendOnlyTreeSnapshot {
    pub root: Fr,
    pub next_available_leaf_index: u32,
}

/// Partial state reference (note hash, nullifier, public data trees).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialStateReference {
    pub note_hash_tree: AppendOnlyTreeSnapshot,
    pub nullifier_tree: AppendOnlyTreeSnapshot,
    pub public_data_tree: AppendOnlyTreeSnapshot,
}

/// Full state reference — L1-to-L2 message tree + partial state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateReference {
    pub l1_to_l2_message_tree: AppendOnlyTreeSnapshot,
    pub partial: PartialStateReference,
}

/// Global variables included in a block header.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalVariables {
    pub chain_id: Fr,
    pub version: Fr,
    pub block_number: u64,
    pub slot_number: u64,
    pub timestamp: u64,
    pub coinbase: EthAddress,
    pub fee_recipient: AztecAddress,
    pub gas_fees: GasFees,
}

/// Block header — the full header of an L2 block.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockHeader {
    pub last_archive: AppendOnlyTreeSnapshot,
    pub state: StateReference,
    pub sponge_blob_hash: Fr,
    pub global_variables: GlobalVariables,
    pub total_fees: Fr,
    pub total_mana_used: Fr,
}

// ---------------------------------------------------------------------------
// Tx constant data
// ---------------------------------------------------------------------------

/// Transaction context — chain ID, version, and gas settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxContext {
    pub chain_id: Fr,
    pub version: Fr,
    pub gas_settings: GasSettings,
}

/// Immutable transaction-wide constants.
///
/// Field ordering matches TS `TxConstantData`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxConstantData {
    pub anchor_block_header: BlockHeader,
    pub tx_context: TxContext,
    pub vk_tree_root: Fr,
    pub protocol_contracts_hash: Fr,
}

// ---------------------------------------------------------------------------
// Tail circuit public inputs
// ---------------------------------------------------------------------------

/// Private kernel tail output for transactions with public calls.
///
/// Field ordering matches TS `PartialPrivateTailPublicInputsForPublic`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialPrivateTailPublicInputsForPublic {
    pub non_revertible_accumulated_data: PrivateToPublicAccumulatedData,
    pub revertible_accumulated_data: PrivateToPublicAccumulatedData,
    pub public_teardown_call_request: PublicCallRequest,
}

/// Private kernel tail output for private-only transactions.
///
/// Field ordering matches TS `PartialPrivateTailPublicInputsForRollup`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialPrivateTailPublicInputsForRollup {
    pub end: PrivateToRollupAccumulatedData,
}

/// The full typed private kernel tail circuit public inputs.
///
/// Exactly one of `for_public` or `for_rollup` will be `Some`.
/// This replaces the opaque buffer wrapper used previously.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateKernelTailPublicInputs {
    pub constants: TxConstantData,
    pub gas_used: Gas,
    pub fee_payer: AztecAddress,
    pub expiration_timestamp: u64,
    pub for_public: Option<PartialPrivateTailPublicInputsForPublic>,
    pub for_rollup: Option<PartialPrivateTailPublicInputsForRollup>,
}

impl PrivateKernelTailPublicInputs {
    /// Returns true if this transaction has enqueued public calls.
    pub fn is_for_public(&self) -> bool {
        self.for_public.is_some()
    }

    /// Collect all non-empty contract class log hashes from this tx.
    pub fn get_non_empty_contract_class_logs_hashes(&self) -> Vec<&ScopedLogHash> {
        let hashes: &[ScopedLogHash] = if let Some(ref for_public) = self.for_public {
            // Combine from both non-revertible and revertible
            // For simplicity, return non-revertible first then revertible
            return for_public
                .non_revertible_accumulated_data
                .contract_class_logs_hashes
                .iter()
                .chain(
                    for_public
                        .revertible_accumulated_data
                        .contract_class_logs_hashes
                        .iter(),
                )
                .filter(|h| !h.is_empty())
                .collect();
        } else if let Some(ref for_rollup) = self.for_rollup {
            &for_rollup.end.contract_class_logs_hashes
        } else {
            return vec![];
        };
        hashes.iter().filter(|h| !h.is_empty()).collect()
    }

    /// Count total number of public call requests.
    pub fn number_of_public_calls(&self) -> usize {
        match &self.for_public {
            Some(for_public) => {
                let nr = for_public
                    .non_revertible_accumulated_data
                    .public_call_requests
                    .iter()
                    .filter(|r| !r.is_empty())
                    .count();
                let r = for_public
                    .revertible_accumulated_data
                    .public_call_requests
                    .iter()
                    .filter(|r| !r.is_empty())
                    .count();
                let td = if for_public.public_teardown_call_request.is_empty() {
                    0
                } else {
                    1
                };
                nr + r + td
            }
            None => 0,
        }
    }

    /// Get all non-empty public call requests in order:
    /// non-revertible setup, revertible app logic, teardown.
    pub fn get_all_public_call_requests(&self) -> Vec<&PublicCallRequest> {
        match &self.for_public {
            Some(for_public) => {
                let mut requests: Vec<&PublicCallRequest> = Vec::new();
                for r in &for_public
                    .non_revertible_accumulated_data
                    .public_call_requests
                {
                    if !r.is_empty() {
                        requests.push(r);
                    }
                }
                for r in &for_public.revertible_accumulated_data.public_call_requests {
                    if !r.is_empty() {
                        requests.push(r);
                    }
                }
                if !for_public.public_teardown_call_request.is_empty() {
                    requests.push(&for_public.public_teardown_call_request);
                }
                requests
            }
            None => vec![],
        }
    }

    /// Flatten into field elements in the same order as upstream stdlib `toFields()`.
    pub fn to_fields(&self) -> Vec<Fr> {
        let mut fields = Vec::new();
        self.constants.write_fields(&mut fields);
        if let Some(ref for_public) = self.for_public {
            for_public.write_fields(&mut fields);
            self.gas_used.write_fields(&mut fields);
            fields.push(self.fee_payer.0);
            fields.push(Fr::from(self.expiration_timestamp));
        } else if let Some(ref for_rollup) = self.for_rollup {
            for_rollup.end.write_fields(&mut fields);
            self.gas_used.write_fields(&mut fields);
            fields.push(self.fee_payer.0);
            fields.push(Fr::from(self.expiration_timestamp));
        }
        fields
    }

    /// Compute the canonical tx hash from these public inputs.
    pub fn hash(&self) -> Fr {
        let separator = if self.is_for_public() {
            domain_separator::PUBLIC_TX_HASH
        } else {
            domain_separator::PRIVATE_TX_HASH
        };
        poseidon2_hash_with_separator(&self.to_fields(), separator)
    }
}

// impl NoteHash {
//     fn write_fields(&self, out: &mut Vec<Fr>) {
//         out.push(self.value);
//         out.push(Fr::from(u64::from(self.counter)));
//     }
// }

// impl ScopedNoteHash {
//     fn write_fields(&self, out: &mut Vec<Fr>) {
//         self.note_hash.write_fields(out);
//         out.push(self.contract_address.0);
//     }
// }

// impl Nullifier {
//     fn write_fields(&self, out: &mut Vec<Fr>) {
//         out.push(self.value);
//         out.push(self.note_hash);
//         out.push(Fr::from(u64::from(self.counter)));
//     }
// }

// impl ScopedNullifier {
//     fn write_fields(&self, out: &mut Vec<Fr>) {
//         self.nullifier.write_fields(out);
//         out.push(self.contract_address.0);
//     }
// }

impl L2ToL1Message {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        out.push(eth_address_to_fr(&self.recipient));
        out.push(self.content);
    }
}

impl ScopedL2ToL1Message {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        self.message.write_fields(out);
        out.push(self.contract_address.0);
    }
}

impl LogHash {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        out.push(self.value);
        out.push(Fr::from(u64::from(self.length)));
    }
}

impl ScopedLogHash {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        self.log_hash.write_fields(out);
        out.push(self.contract_address.0);
    }
}

impl PrivateLog {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        out.extend(self.fields.iter().copied());
        out.push(Fr::from(u64::from(self.emitted_length)));
    }
}

impl PublicCallRequest {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        out.push(self.msg_sender.0);
        out.push(self.contract_address.0);
        out.push(Fr::from(self.is_static_call));
        out.push(self.calldata_hash);
    }
}

impl PrivateToPublicAccumulatedData {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        out.extend(self.note_hashes.iter().copied());
        out.extend(self.nullifiers.iter().copied());
        for msg in &self.l2_to_l1_msgs {
            msg.write_fields(out);
        }
        for log in &self.private_logs {
            log.write_fields(out);
        }
        for hash in &self.contract_class_logs_hashes {
            hash.write_fields(out);
        }
        for req in &self.public_call_requests {
            req.write_fields(out);
        }
    }
}

impl PrivateToRollupAccumulatedData {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        out.extend(self.note_hashes.iter().copied());
        out.extend(self.nullifiers.iter().copied());
        for msg in &self.l2_to_l1_msgs {
            msg.write_fields(out);
        }
        for log in &self.private_logs {
            log.write_fields(out);
        }
        for hash in &self.contract_class_logs_hashes {
            hash.write_fields(out);
        }
    }
}

impl AppendOnlyTreeSnapshot {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        out.push(self.root);
        out.push(Fr::from(u64::from(self.next_available_leaf_index)));
    }
}

impl PartialStateReference {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        self.note_hash_tree.write_fields(out);
        self.nullifier_tree.write_fields(out);
        self.public_data_tree.write_fields(out);
    }
}

impl StateReference {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        self.l1_to_l2_message_tree.write_fields(out);
        self.partial.write_fields(out);
    }
}

impl GlobalVariables {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        out.push(self.chain_id);
        out.push(self.version);
        out.push(Fr::from(self.block_number));
        out.push(Fr::from(self.slot_number));
        out.push(Fr::from(self.timestamp));
        out.push(eth_address_to_fr(&self.coinbase));
        out.push(self.fee_recipient.0);
        self.gas_fees.write_fields(out);
    }
}

impl BlockHeader {
    /// Flatten into field elements using the canonical stdlib ordering.
    pub fn to_fields(&self) -> Vec<Fr> {
        let mut fields = Vec::new();
        self.write_fields(&mut fields);
        fields
    }

    fn write_fields(&self, out: &mut Vec<Fr>) {
        self.last_archive.write_fields(out);
        self.state.write_fields(out);
        out.push(self.sponge_blob_hash);
        self.global_variables.write_fields(out);
        out.push(self.total_fees);
        out.push(self.total_mana_used);
    }
}

impl TxContext {
    /// Flatten into field elements using the canonical stdlib ordering.
    pub fn to_fields(&self) -> Vec<Fr> {
        let mut fields = Vec::new();
        self.write_fields(&mut fields);
        fields
    }

    fn write_fields(&self, out: &mut Vec<Fr>) {
        out.push(self.chain_id);
        out.push(self.version);
        self.gas_settings.write_fields(out);
    }
}

impl TxConstantData {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        self.anchor_block_header.write_fields(out);
        self.tx_context.write_fields(out);
        out.push(self.vk_tree_root);
        out.push(self.protocol_contracts_hash);
    }
}

impl PartialPrivateTailPublicInputsForPublic {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        self.non_revertible_accumulated_data.write_fields(out);
        self.revertible_accumulated_data.write_fields(out);
        self.public_teardown_call_request.write_fields(out);
    }
}

impl Gas {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        out.push(Fr::from(self.da_gas));
        out.push(Fr::from(self.l2_gas));
    }
}

impl GasFees {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        out.push(Fr::from(self.fee_per_da_gas));
        out.push(Fr::from(self.fee_per_l2_gas));
    }
}

impl GasSettings {
    fn write_fields(&self, out: &mut Vec<Fr>) {
        self.gas_limits
            .clone()
            .unwrap_or_default()
            .write_fields(out);
        self.teardown_gas_limits
            .clone()
            .unwrap_or_default()
            .write_fields(out);
        self.max_fee_per_gas
            .clone()
            .unwrap_or_default()
            .write_fields(out);
        self.max_priority_fee_per_gas
            .clone()
            .unwrap_or_default()
            .write_fields(out);
    }
}

impl CallContext {
    /// Flatten into field elements using the canonical stdlib ordering.
    pub fn to_fields(&self) -> Vec<Fr> {
        vec![
            self.msg_sender.0,
            self.contract_address.0,
            self.function_selector,
            Fr::from(self.is_static_call),
        ]
    }
}

// ---------------------------------------------------------------------------
// Buffer serialization
// ---------------------------------------------------------------------------
//
// The wire format must match the TS stdlib `serializeToBuffer` output exactly.
// TS uses mixed sizes: bool=1B, u32=4B, u64=8B, u128=16B, EthAddress=20B, Fr=32B.

/// Append a field element as 32-byte big-endian.
fn write_fr(buf: &mut Vec<u8>, fr: &Fr) {
    buf.extend_from_slice(&fr.to_be_bytes());
}

/// Append a boolean as 1 byte.
fn write_bool(buf: &mut Vec<u8>, v: bool) {
    buf.push(u8::from(v));
}

/// Append a u32 as 4-byte big-endian (matches TS `readNumber` / `serializeToBuffer(number)`).
fn write_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_be_bytes());
}

/// Append a u64 as 8-byte big-endian (matches TS `bigintToUInt64BE`).
fn write_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_be_bytes());
}

/// Append a u128 as 16-byte big-endian (matches TS `bigintToUInt128BE`).
fn write_u128(buf: &mut Vec<u8>, v: u128) {
    buf.extend_from_slice(&v.to_be_bytes());
}

/// Append an EthAddress as 20 raw bytes.
fn write_eth_address(buf: &mut Vec<u8>, addr: &EthAddress) {
    buf.extend_from_slice(&addr.0);
}

impl PrivateKernelTailPublicInputs {
    /// Serialize to the buffer format matching upstream TS stdlib.
    ///
    /// Field sizes must exactly match the TS `serializeToBuffer` output:
    /// bool=1B, u32=4B, u64=8B, u128=16B, EthAddress=20B, Fr/AztecAddress=32B.
    pub fn to_buffer(&self) -> Vec<u8> {
        let is_for_public = self.for_public.is_some();
        let mut buf = Vec::with_capacity(if is_for_public { 91_000 } else { 43_000 });

        // Discriminator: boolean (1 byte)
        write_bool(&mut buf, is_for_public);

        // TxConstantData
        self.write_tx_constant_data(&mut buf);

        // gas_used: Gas (2 × u32 = 8 bytes)
        write_u32(&mut buf, self.gas_used.da_gas as u32);
        write_u32(&mut buf, self.gas_used.l2_gas as u32);

        // fee_payer: AztecAddress (32 bytes)
        write_fr(&mut buf, &self.fee_payer.0);

        // expiration_timestamp: UInt64 (8 bytes)
        write_u64(&mut buf, self.expiration_timestamp);

        // Variant-specific accumulated data
        if let Some(ref for_public) = self.for_public {
            Self::write_public_accumulated_data(
                &mut buf,
                &for_public.non_revertible_accumulated_data,
            );
            Self::write_public_accumulated_data(&mut buf, &for_public.revertible_accumulated_data);
            Self::write_public_call_request(&mut buf, &for_public.public_teardown_call_request);
        } else if let Some(ref for_rollup) = self.for_rollup {
            Self::write_rollup_accumulated_data(&mut buf, &for_rollup.end);
        }

        buf
    }

    fn write_tree_snapshot(buf: &mut Vec<u8>, snap: &AppendOnlyTreeSnapshot) {
        write_fr(buf, &snap.root); // Fr: 32 bytes
        write_u32(buf, snap.next_available_leaf_index); // u32: 4 bytes
    }

    fn write_tx_constant_data(&self, buf: &mut Vec<u8>) {
        let c = &self.constants;

        // BlockHeader
        let h = &c.anchor_block_header;
        // last_archive: AppendOnlyTreeSnapshot (36 bytes)
        Self::write_tree_snapshot(buf, &h.last_archive);
        // state.l1_to_l2_message_tree
        Self::write_tree_snapshot(buf, &h.state.l1_to_l2_message_tree);
        // state.partial.note_hash_tree
        Self::write_tree_snapshot(buf, &h.state.partial.note_hash_tree);
        // state.partial.nullifier_tree
        Self::write_tree_snapshot(buf, &h.state.partial.nullifier_tree);
        // state.partial.public_data_tree
        Self::write_tree_snapshot(buf, &h.state.partial.public_data_tree);
        // sponge_blob_hash: Fr (32 bytes)
        write_fr(buf, &h.sponge_blob_hash);
        // global_variables
        write_fr(buf, &h.global_variables.chain_id); // Fr: 32 bytes
        write_fr(buf, &h.global_variables.version); // Fr: 32 bytes
        write_u32(buf, h.global_variables.block_number as u32); // u32: 4 bytes
        write_u32(buf, h.global_variables.slot_number as u32); // u32: 4 bytes
        write_u64(buf, h.global_variables.timestamp); // u64: 8 bytes
        write_eth_address(buf, &h.global_variables.coinbase); // EthAddress: 20 bytes
        write_fr(buf, &h.global_variables.fee_recipient.0); // AztecAddress: 32 bytes
                                                            // gasFees: GasFees (2 × u128 = 32 bytes)
        write_u128(buf, h.global_variables.gas_fees.fee_per_da_gas);
        write_u128(buf, h.global_variables.gas_fees.fee_per_l2_gas);
        // total_fees, total_mana_used: Fr (32 bytes each)
        write_fr(buf, &h.total_fees);
        write_fr(buf, &h.total_mana_used);

        // TxContext
        write_fr(buf, &c.tx_context.chain_id); // Fr: 32 bytes
        write_fr(buf, &c.tx_context.version); // Fr: 32 bytes
                                              // GasSettings: gasLimits (Gas 8B) + teardownGasLimits (Gas 8B) +
                                              //              maxFeesPerGas (GasFees 32B) + maxPriorityFeesPerGas (GasFees 32B) = 80B
        let gs = &c.tx_context.gas_settings;
        let gl = gs.gas_limits.as_ref();
        write_u32(buf, gl.map_or(0, |g| g.da_gas as u32));
        write_u32(buf, gl.map_or(0, |g| g.l2_gas as u32));
        let tl = gs.teardown_gas_limits.as_ref();
        write_u32(buf, tl.map_or(0, |g| g.da_gas as u32));
        write_u32(buf, tl.map_or(0, |g| g.l2_gas as u32));
        let mf = gs.max_fee_per_gas.as_ref();
        write_u128(buf, mf.map_or(0, |g| g.fee_per_da_gas));
        write_u128(buf, mf.map_or(0, |g| g.fee_per_l2_gas));
        let mp = gs.max_priority_fee_per_gas.as_ref();
        write_u128(buf, mp.map_or(0, |g| g.fee_per_da_gas));
        write_u128(buf, mp.map_or(0, |g| g.fee_per_l2_gas));

        // vk_tree_root, protocol_contracts_hash: Fr (32 bytes each)
        write_fr(buf, &c.vk_tree_root);
        write_fr(buf, &c.protocol_contracts_hash);
    }

    fn write_rollup_accumulated_data(buf: &mut Vec<u8>, data: &PrivateToRollupAccumulatedData) {
        // note_hashes: 64 × Fr
        for h in &data.note_hashes {
            write_fr(buf, h);
        }
        // nullifiers: 64 × Fr
        for n in &data.nullifiers {
            write_fr(buf, n);
        }
        // l2_to_l1_msgs: 8 × ScopedL2ToL1Message (EthAddress(20) + Fr(32) + Fr(32) = 84 each)
        for msg in &data.l2_to_l1_msgs {
            write_eth_address(buf, &msg.message.recipient);
            write_fr(buf, &msg.message.content);
            write_fr(buf, &msg.contract_address.0);
        }
        // private_logs: 64 × PrivateLog (18 × Fr(32) + u32(4) = 580 each)
        for log in &data.private_logs {
            for f in &log.fields {
                write_fr(buf, f);
            }
            write_u32(buf, log.emitted_length);
        }
        // contract_class_logs_hashes: 1 × ScopedLogHash (Fr(32) + u32(4) + Fr(32) = 68 each)
        for lh in &data.contract_class_logs_hashes {
            write_fr(buf, &lh.log_hash.value);
            write_u32(buf, lh.log_hash.length);
            write_fr(buf, &lh.contract_address.0);
        }
    }

    fn write_public_accumulated_data(buf: &mut Vec<u8>, data: &PrivateToPublicAccumulatedData) {
        // note_hashes: 64 × Fr
        for h in &data.note_hashes {
            write_fr(buf, h);
        }
        // nullifiers: 64 × Fr
        for n in &data.nullifiers {
            write_fr(buf, n);
        }
        // l2_to_l1_msgs: 8 × ScopedL2ToL1Message
        for msg in &data.l2_to_l1_msgs {
            write_eth_address(buf, &msg.message.recipient);
            write_fr(buf, &msg.message.content);
            write_fr(buf, &msg.contract_address.0);
        }
        // private_logs: 64 × PrivateLog
        for log in &data.private_logs {
            for f in &log.fields {
                write_fr(buf, f);
            }
            write_u32(buf, log.emitted_length);
        }
        // contract_class_logs_hashes: 1 × ScopedLogHash
        for lh in &data.contract_class_logs_hashes {
            write_fr(buf, &lh.log_hash.value);
            write_u32(buf, lh.log_hash.length);
            write_fr(buf, &lh.contract_address.0);
        }
        // public_call_requests: 32 × PublicCallRequest (32+32+1+32 = 97 each)
        for req in &data.public_call_requests {
            Self::write_public_call_request(buf, req);
        }
    }

    fn write_public_call_request(buf: &mut Vec<u8>, req: &PublicCallRequest) {
        write_fr(buf, &req.msg_sender.0); // AztecAddress: 32 bytes
        write_fr(buf, &req.contract_address.0); // AztecAddress: 32 bytes
        write_bool(buf, req.is_static_call); // bool: 1 byte
        write_fr(buf, &req.calldata_hash); // Fr: 32 bytes
    }
}

// ---------------------------------------------------------------------------
// Contract class log
// ---------------------------------------------------------------------------

/// A contract class log with its emitting contract and emitted length.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractClassLog {
    pub contract_address: AztecAddress,
    pub fields: Vec<Fr>,
    pub emitted_length: u32,
}

/// A contract class log with a counter for ordering.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CountedContractClassLog {
    pub log: ContractClassLog,
    pub counter: u32,
}

// ---------------------------------------------------------------------------
// Read requests
// ---------------------------------------------------------------------------

/// A read request — value with a side-effect counter.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadRequest {
    pub value: Fr,
    pub counter: u32,
}

/// A read request scoped to a contract address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopedReadRequest {
    pub read_request: ReadRequest,
    pub contract_address: AztecAddress,
}

// ---------------------------------------------------------------------------
// Note and slot
// ---------------------------------------------------------------------------

/// A note captured during execution with its storage slot metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteAndSlot {
    pub contract_address: AztecAddress,
    pub owner: AztecAddress,
    pub storage_slot: Fr,
    pub randomness: Fr,
    pub note_type_id: Fr,
    pub note_items: Vec<Fr>,
    pub note_hash: Fr,
    pub counter: u32,
}

// ---------------------------------------------------------------------------
// Call context
// ---------------------------------------------------------------------------

/// Execution call context for private functions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallContext {
    pub msg_sender: AztecAddress,
    pub contract_address: AztecAddress,
    pub function_selector: Fr,
    pub is_static_call: bool,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Pad a vector to a fixed length with a default value.
pub fn pad_to<T: Clone>(mut vec: Vec<T>, len: usize, default: T) -> Vec<T> {
    if vec.len() < len {
        vec.resize(len, default);
    }
    vec
}

/// Pad a field vector to a fixed length with zeros.
pub fn pad_fields(mut vec: Vec<Fr>, len: usize) -> Vec<Fr> {
    if vec.len() < len {
        vec.resize(len, Fr::zero());
    }
    vec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_kernel_tail_public_inputs_serialization() {
        let pi = PrivateKernelTailPublicInputs {
            constants: TxConstantData::default(),
            gas_used: Gas::default(),
            fee_payer: AztecAddress::zero(),
            expiration_timestamp: 0,
            for_public: None,
            for_rollup: Some(PartialPrivateTailPublicInputsForRollup {
                end: PrivateToRollupAccumulatedData::default(),
            }),
        };
        assert!(!pi.is_for_public());
        assert_eq!(pi.number_of_public_calls(), 0);
    }

    #[test]
    fn public_call_count() {
        let mut pi = PrivateKernelTailPublicInputs::default();
        pi.for_public = Some(PartialPrivateTailPublicInputsForPublic {
            non_revertible_accumulated_data: PrivateToPublicAccumulatedData {
                public_call_requests: vec![PublicCallRequest {
                    contract_address: AztecAddress(Fr::from(1u64)),
                    ..Default::default()
                }],
                ..Default::default()
            },
            revertible_accumulated_data: PrivateToPublicAccumulatedData {
                public_call_requests: vec![PublicCallRequest {
                    contract_address: AztecAddress(Fr::from(2u64)),
                    ..Default::default()
                }],
                ..Default::default()
            },
            public_teardown_call_request: PublicCallRequest {
                contract_address: AztecAddress(Fr::from(3u64)),
                ..Default::default()
            },
        });
        assert!(pi.is_for_public());
        assert_eq!(pi.number_of_public_calls(), 3);
    }

    #[test]
    fn pad_fields_works() {
        let v = vec![Fr::from(1u64), Fr::from(2u64)];
        let padded = pad_fields(v, 5);
        assert_eq!(padded.len(), 5);
        assert_eq!(padded[0], Fr::from(1u64));
        assert_eq!(padded[4], Fr::zero());
    }

    #[test]
    fn to_buffer_rollup_size() {
        let pi = PrivateKernelTailPublicInputs {
            for_public: None,
            for_rollup: Some(PartialPrivateTailPublicInputsForRollup {
                end: PrivateToRollupAccumulatedData::empty(),
            }),
            ..Default::default()
        };
        // TS mixed sizes:
        // bool(1) + TxConstantData(648) + Gas(8) + AztecAddress(32) + u64(8) + rollup_data(37860)
        // rollup_data = 64*32 + 64*32 + 8*(20+32+32) + 64*(16*32+4) + 1*(32+4+32)
        //             = 2048 + 2048 + 672 + 33024 + 68 = 37860
        assert_eq!(pi.to_buffer().len(), 38557);
    }

    #[test]
    fn to_buffer_public_size() {
        let pi = PrivateKernelTailPublicInputs {
            for_public: Some(PartialPrivateTailPublicInputsForPublic {
                non_revertible_accumulated_data: PrivateToPublicAccumulatedData::empty(),
                revertible_accumulated_data: PrivateToPublicAccumulatedData::empty(),
                public_teardown_call_request: PublicCallRequest::empty(),
            }),
            for_rollup: None,
            ..Default::default()
        };
        // public_data = rollup_data(37860) + 32*(32+32+1+32)(=3104) = 40964
        // total = bool(1) + TxConstantData(648) + Gas(8) + AztecAddress(32) + u64(8)
        //       + 2*public_data(81928) + PublicCallRequest(97) = 82722
        assert_eq!(pi.to_buffer().len(), 82722);
    }
}
