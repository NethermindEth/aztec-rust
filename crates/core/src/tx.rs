use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

use crate::abi::{AbiValue, FunctionSelector, FunctionType};
use crate::fee::GasSettings;
use crate::types::{decode_fixed_hex, encode_hex, AztecAddress, Fr};
use crate::Error;

/// A 32-byte transaction hash.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TxHash(pub [u8; 32]);

impl TxHash {
    /// The zero hash.
    pub const fn zero() -> Self {
        Self([0u8; 32])
    }

    /// Parse from a hex string (e.g. `"0xdead..."`).
    pub fn from_hex(value: &str) -> Result<Self, Error> {
        Ok(Self(decode_fixed_hex::<32>(value)?))
    }
}

impl fmt::Display for TxHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&encode_hex(&self.0))
    }
}

impl fmt::Debug for TxHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TxHash({self})")
    }
}

impl Serialize for TxHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for TxHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

/// Transaction lifecycle status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TxStatus {
    /// Transaction was dropped from the mempool.
    Dropped,
    /// Transaction is pending in the mempool.
    Pending,
    /// Transaction has been proposed in a block.
    Proposed,
    /// Transaction's block has been checkpointed to L1.
    Checkpointed,
    /// Transaction's block has been proven.
    Proven,
    /// Transaction's block has been finalized on L1.
    Finalized,
}

/// Outcome of transaction execution within a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxExecutionResult {
    /// All phases executed successfully.
    Success,
    /// The app logic phase reverted.
    AppLogicReverted,
    /// The teardown phase reverted.
    TeardownReverted,
    /// Both app logic and teardown phases reverted.
    BothReverted,
}

/// A transaction receipt returned by the node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxReceipt {
    /// Hash of the transaction.
    pub tx_hash: TxHash,
    /// Current lifecycle status.
    pub status: TxStatus,
    /// Execution outcome (present once the tx has been included in a block).
    pub execution_result: Option<TxExecutionResult>,
    /// Error message if the transaction failed.
    pub error: Option<String>,
    /// Total fee paid for the transaction.
    pub transaction_fee: Option<u128>,
    /// Hash of the block containing this transaction.
    #[serde(default, with = "option_hex_bytes_32")]
    pub block_hash: Option<[u8; 32]>,
    /// Block number containing this transaction.
    pub block_number: Option<u64>,
    /// Epoch number containing this transaction.
    pub epoch_number: Option<u64>,
}

mod option_hex_bytes_32 {
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::types::{decode_fixed_hex, encode_hex};

    #[allow(clippy::ref_option)]
    pub fn serialize<S>(value: &Option<[u8; 32]>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(bytes) => serializer.serialize_some(&encode_hex(bytes)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<[u8; 32]>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<String> = Option::deserialize(deserializer)?;
        match opt {
            Some(s) => {
                let bytes = decode_fixed_hex::<32>(&s).map_err(serde::de::Error::custom)?;
                Ok(Some(bytes))
            }
            None => Ok(None),
        }
    }
}

impl TxReceipt {
    /// Returns `true` if the transaction has been included in a block.
    pub const fn is_mined(&self) -> bool {
        matches!(
            self.status,
            TxStatus::Proposed | TxStatus::Checkpointed | TxStatus::Proven | TxStatus::Finalized
        )
    }

    /// Returns `true` if the transaction is pending in the mempool.
    pub fn is_pending(&self) -> bool {
        self.status == TxStatus::Pending
    }

    /// Returns `true` if the transaction was dropped from the mempool.
    pub fn is_dropped(&self) -> bool {
        self.status == TxStatus::Dropped
    }

    /// Returns `true` if execution completed successfully.
    pub fn has_execution_succeeded(&self) -> bool {
        self.execution_result == Some(TxExecutionResult::Success)
    }

    /// Returns `true` if any execution phase reverted.
    pub fn has_execution_reverted(&self) -> bool {
        self.execution_result.is_some() && !self.has_execution_succeeded()
    }
}

/// A single function call to a contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionCall {
    /// Target contract address.
    pub to: AztecAddress,
    /// Function selector identifying the function to call.
    pub selector: FunctionSelector,
    /// Encoded function arguments.
    pub args: Vec<AbiValue>,
    /// The type of function being called.
    pub function_type: FunctionType,
    /// Whether this is a static (read-only) call.
    pub is_static: bool,
    /// Whether the msg_sender should be hidden from the callee.
    #[serde(default)]
    pub hide_msg_sender: bool,
}

impl FunctionCall {
    /// The canonical empty function call, used for padding entrypoint payloads.
    pub fn empty() -> Self {
        Self {
            to: AztecAddress::zero(),
            selector: FunctionSelector::empty(),
            args: vec![],
            function_type: FunctionType::Private,
            is_static: false,
            hide_msg_sender: false,
        }
    }

    /// Returns `true` if this is the canonical empty call.
    pub fn is_empty(&self) -> bool {
        self.to == AztecAddress::zero() && self.selector == FunctionSelector::empty()
    }
}

/// An authorization witness proving the caller's intent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuthWitness {
    /// The message hash this witness authorizes.
    #[serde(default)]
    pub request_hash: Fr,
    /// Field elements comprising the witness data.
    #[serde(default)]
    pub fields: Vec<Fr>,
}

/// Private data capsule passed alongside a transaction.
///
/// Structured capsule with contract address, storage slot, and field data.
/// Used for passing auxiliary data (e.g., packed bytecode) to protocol contracts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capsule {
    /// The contract address this capsule targets.
    pub contract_address: AztecAddress,
    /// The storage slot within the target contract.
    pub storage_slot: Fr,
    /// Capsule data as field elements.
    pub data: Vec<Fr>,
}

/// Transaction context.
///
/// Carries the replay-protection metadata and gas settings used to build a
/// transaction execution request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxContext {
    /// L1 chain ID used for replay protection.
    pub chain_id: Fr,
    /// Rollup protocol version used for replay protection.
    pub version: Fr,
    /// Gas settings for the transaction.
    pub gas_settings: GasSettings,
}

/// Pre-hashed values included in a transaction for oracle access.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HashedValues {
    /// Field elements to be hashed.
    #[serde(default)]
    pub values: Vec<Fr>,
    /// Pre-computed hash of `values`.
    #[serde(default)]
    pub hash: Fr,
}

impl HashedValues {
    /// Create hashed values from raw argument fields.
    pub fn from_args(args: Vec<Fr>) -> Self {
        let hash = crate::hash::compute_var_args_hash(&args);
        Self { values: args, hash }
    }

    /// Create hashed values from calldata (selector + args for public calls).
    pub fn from_calldata(calldata: Vec<Fr>) -> Self {
        let hash = crate::hash::compute_calldata_hash(&calldata);
        Self {
            values: calldata,
            hash,
        }
    }

    /// Return the stored hash of the contained values.
    pub fn hash(&self) -> Fr {
        self.hash
    }
}

/// A complete transaction execution payload.
///
/// Groups one or more [`FunctionCall`]s with their associated auth witnesses,
/// capsules, hashed args, and an optional fee payer override.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExecutionPayload {
    /// Function calls to execute.
    #[serde(default)]
    pub calls: Vec<FunctionCall>,
    /// Authorization witnesses for the calls.
    #[serde(default)]
    pub auth_witnesses: Vec<AuthWitness>,
    /// Private data capsules.
    #[serde(default)]
    pub capsules: Vec<Capsule>,
    /// Extra hashed arguments for oracle access.
    #[serde(default)]
    pub extra_hashed_args: Vec<HashedValues>,
    /// Override the fee payer for this payload.
    pub fee_payer: Option<AztecAddress>,
}

impl ExecutionPayload {
    /// Merge multiple execution payloads into a single payload.
    ///
    /// Combines all calls, auth witnesses, capsules, and hashed args.
    /// If multiple payloads specify a `fee_payer`, they must all agree
    /// on the same address — otherwise this returns an error.
    pub fn merge(payloads: Vec<ExecutionPayload>) -> Result<Self, Error> {
        let mut calls = Vec::new();
        let mut auth_witnesses = Vec::new();
        let mut capsules = Vec::new();
        let mut extra_hashed_args = Vec::new();
        let mut fee_payer: Option<AztecAddress> = None;

        for payload in payloads {
            calls.extend(payload.calls);
            auth_witnesses.extend(payload.auth_witnesses);
            capsules.extend(payload.capsules);
            extra_hashed_args.extend(payload.extra_hashed_args);

            if let Some(payer) = payload.fee_payer {
                if let Some(existing) = fee_payer {
                    if existing != payer {
                        return Err(Error::InvalidData(format!(
                            "conflicting fee payers: {existing} vs {payer}"
                        )));
                    }
                }
                fee_payer = Some(payer);
            }
        }

        Ok(ExecutionPayload {
            calls,
            auth_witnesses,
            capsules,
            extra_hashed_args,
            fee_payer,
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn make_receipt(status: TxStatus, exec: Option<TxExecutionResult>) -> TxReceipt {
        TxReceipt {
            tx_hash: TxHash::zero(),
            status,
            execution_result: exec,
            error: None,
            transaction_fee: None,
            block_hash: None,
            block_number: None,
            epoch_number: None,
        }
    }

    #[test]
    fn tx_hash_hex_roundtrip() {
        let hash = TxHash([0xab; 32]);
        let json = serde_json::to_string(&hash).expect("serialize TxHash");
        assert!(json.contains("0xabab"), "should serialize as hex string");
        let decoded: TxHash = serde_json::from_str(&json).expect("deserialize TxHash");
        assert_eq!(decoded, hash);
    }

    #[test]
    fn tx_hash_from_hex() {
        let hash =
            TxHash::from_hex("0x0000000000000000000000000000000000000000000000000000000000000001")
                .expect("valid hex");
        assert_eq!(hash.0[31], 1);
        assert_eq!(hash.0[0], 0);
    }

    #[test]
    fn tx_hash_display() {
        let hash = TxHash::zero();
        let s = hash.to_string();
        assert_eq!(
            s,
            "0x0000000000000000000000000000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn tx_status_roundtrip() {
        let statuses = [
            (TxStatus::Dropped, "\"dropped\""),
            (TxStatus::Pending, "\"pending\""),
            (TxStatus::Proposed, "\"proposed\""),
            (TxStatus::Checkpointed, "\"checkpointed\""),
            (TxStatus::Proven, "\"proven\""),
            (TxStatus::Finalized, "\"finalized\""),
        ];

        for (status, expected_json) in statuses {
            let json = serde_json::to_string(&status).expect("serialize TxStatus");
            assert_eq!(json, expected_json);
            let decoded: TxStatus = serde_json::from_str(&json).expect("deserialize TxStatus");
            assert_eq!(decoded, status);
        }
    }

    #[test]
    fn tx_execution_result_roundtrip() {
        let results = [
            TxExecutionResult::Success,
            TxExecutionResult::AppLogicReverted,
            TxExecutionResult::TeardownReverted,
            TxExecutionResult::BothReverted,
        ];

        for result in results {
            let json = serde_json::to_string(&result).expect("serialize TxExecutionResult");
            let decoded: TxExecutionResult =
                serde_json::from_str(&json).expect("deserialize TxExecutionResult");
            assert_eq!(decoded, result);
        }
    }

    #[test]
    fn receipt_mined_success() {
        let receipt = TxReceipt {
            tx_hash: TxHash::zero(),
            status: TxStatus::Checkpointed,
            execution_result: Some(TxExecutionResult::Success),
            error: None,
            transaction_fee: Some(1000),
            block_hash: Some([0x11; 32]),
            block_number: Some(42),
            epoch_number: Some(1),
        };

        assert!(receipt.is_mined());
        assert!(!receipt.is_pending());
        assert!(!receipt.is_dropped());
        assert!(receipt.has_execution_succeeded());
        assert!(!receipt.has_execution_reverted());
    }

    #[test]
    fn receipt_pending() {
        let receipt = make_receipt(TxStatus::Pending, None);
        assert!(!receipt.is_mined());
        assert!(receipt.is_pending());
        assert!(!receipt.is_dropped());
        assert!(!receipt.has_execution_succeeded());
        assert!(!receipt.has_execution_reverted());
    }

    #[test]
    fn receipt_dropped() {
        let receipt = make_receipt(TxStatus::Dropped, None);
        assert!(!receipt.is_mined());
        assert!(!receipt.is_pending());
        assert!(receipt.is_dropped());
    }

    #[test]
    fn receipt_reverted() {
        let receipt = make_receipt(
            TxStatus::Checkpointed,
            Some(TxExecutionResult::AppLogicReverted),
        );
        assert!(receipt.is_mined());
        assert!(!receipt.has_execution_succeeded());
        assert!(receipt.has_execution_reverted());
    }

    #[test]
    fn receipt_both_reverted() {
        let receipt = make_receipt(
            TxStatus::Checkpointed,
            Some(TxExecutionResult::BothReverted),
        );
        assert!(receipt.has_execution_reverted());
    }

    #[test]
    fn receipt_all_mined_statuses() {
        for status in [
            TxStatus::Proposed,
            TxStatus::Checkpointed,
            TxStatus::Proven,
            TxStatus::Finalized,
        ] {
            let receipt = make_receipt(status, Some(TxExecutionResult::Success));
            assert!(receipt.is_mined(), "{status:?} should count as mined");
        }
    }

    #[test]
    fn receipt_json_roundtrip() {
        let receipt = TxReceipt {
            tx_hash: TxHash::from_hex(
                "0x00000000000000000000000000000000000000000000000000000000deadbeef",
            )
            .expect("valid hex"),
            status: TxStatus::Finalized,
            execution_result: Some(TxExecutionResult::Success),
            error: None,
            transaction_fee: Some(5000),
            block_hash: Some([0xcc; 32]),
            block_number: Some(100),
            epoch_number: Some(10),
        };

        let json = serde_json::to_string(&receipt).expect("serialize receipt");
        assert!(json.contains("deadbeef"), "tx_hash should be hex");
        assert!(json.contains("0xcc"), "block_hash should be hex");

        let decoded: TxReceipt = serde_json::from_str(&json).expect("deserialize receipt");
        assert_eq!(decoded, receipt);
    }

    #[test]
    fn receipt_json_roundtrip_with_nulls() {
        let receipt = TxReceipt {
            tx_hash: TxHash::zero(),
            status: TxStatus::Pending,
            execution_result: None,
            error: None,
            transaction_fee: None,
            block_hash: None,
            block_number: None,
            epoch_number: None,
        };

        let json = serde_json::to_string(&receipt).expect("serialize receipt");
        let decoded: TxReceipt = serde_json::from_str(&json).expect("deserialize receipt");
        assert_eq!(decoded, receipt);
    }

    #[test]
    fn payload_serializes() {
        let payload = ExecutionPayload::default();
        let json = serde_json::to_string(&payload).expect("serialize ExecutionPayload");
        assert!(json.contains("\"calls\":[]"));
    }

    #[test]
    fn merge_empty_payloads() {
        let result = ExecutionPayload::merge(vec![]).expect("merge empty");
        assert!(result.calls.is_empty());
        assert!(result.auth_witnesses.is_empty());
        assert!(result.capsules.is_empty());
        assert!(result.extra_hashed_args.is_empty());
        assert!(result.fee_payer.is_none());
    }

    #[test]
    fn merge_single_payload() {
        let payer = AztecAddress(Fr::from(1u64));
        let payload = ExecutionPayload {
            calls: vec![FunctionCall {
                to: AztecAddress(Fr::from(2u64)),
                selector: FunctionSelector::from_hex("0x11223344").expect("valid"),
                args: vec![],
                function_type: FunctionType::Private,
                is_static: false,
                hide_msg_sender: false,
            }],
            auth_witnesses: vec![AuthWitness {
                fields: vec![Fr::from(9u64)],
                ..Default::default()
            }],
            capsules: vec![],
            extra_hashed_args: vec![],
            fee_payer: Some(payer),
        };

        let merged = ExecutionPayload::merge(vec![payload]).expect("merge single");
        assert_eq!(merged.calls.len(), 1);
        assert_eq!(merged.fee_payer, Some(payer));
    }

    #[test]
    fn merge_concatenates_fields() {
        let p1 = ExecutionPayload {
            calls: vec![FunctionCall {
                to: AztecAddress(Fr::from(1u64)),
                selector: FunctionSelector::from_hex("0x11111111").expect("valid"),
                args: vec![],
                function_type: FunctionType::Private,
                is_static: false,
                hide_msg_sender: false,
            }],
            auth_witnesses: vec![AuthWitness {
                fields: vec![Fr::from(1u64)],
                ..Default::default()
            }],
            capsules: vec![],
            extra_hashed_args: vec![],
            fee_payer: None,
        };

        let p2 = ExecutionPayload {
            calls: vec![FunctionCall {
                to: AztecAddress(Fr::from(2u64)),
                selector: FunctionSelector::from_hex("0x22222222").expect("valid"),
                args: vec![],
                function_type: FunctionType::Public,
                is_static: false,
                hide_msg_sender: false,
            }],
            auth_witnesses: vec![AuthWitness {
                fields: vec![Fr::from(2u64)],
                ..Default::default()
            }],
            capsules: vec![],
            extra_hashed_args: vec![],
            fee_payer: None,
        };

        let merged = ExecutionPayload::merge(vec![p1, p2]).expect("merge two");
        assert_eq!(merged.calls.len(), 2);
        assert_eq!(merged.auth_witnesses.len(), 2);
        assert!(merged.fee_payer.is_none());
    }

    #[test]
    fn merge_same_fee_payer_succeeds() {
        let payer = AztecAddress(Fr::from(5u64));
        let p1 = ExecutionPayload {
            fee_payer: Some(payer),
            ..Default::default()
        };
        let p2 = ExecutionPayload {
            fee_payer: Some(payer),
            ..Default::default()
        };

        let merged = ExecutionPayload::merge(vec![p1, p2]).expect("same payer");
        assert_eq!(merged.fee_payer, Some(payer));
    }

    #[test]
    fn merge_conflicting_fee_payer_errors() {
        let p1 = ExecutionPayload {
            fee_payer: Some(AztecAddress(Fr::from(1u64))),
            ..Default::default()
        };
        let p2 = ExecutionPayload {
            fee_payer: Some(AztecAddress(Fr::from(2u64))),
            ..Default::default()
        };

        let result = ExecutionPayload::merge(vec![p1, p2]);
        assert!(result.is_err());
    }

    #[test]
    fn merge_mixed_fee_payer_takes_defined() {
        let payer = AztecAddress(Fr::from(3u64));
        let p1 = ExecutionPayload {
            fee_payer: None,
            ..Default::default()
        };
        let p2 = ExecutionPayload {
            fee_payer: Some(payer),
            ..Default::default()
        };

        let merged = ExecutionPayload::merge(vec![p1, p2]).expect("mixed payer");
        assert_eq!(merged.fee_payer, Some(payer));
    }

    #[test]
    fn payload_with_calls_roundtrip() {
        let payload = ExecutionPayload {
            calls: vec![FunctionCall {
                to: AztecAddress(Fr::from(1u64)),
                selector: crate::abi::FunctionSelector::from_hex("0xaabbccdd")
                    .expect("valid selector"),
                args: vec![AbiValue::Field(Fr::from(42u64))],
                function_type: FunctionType::Private,
                is_static: false,
                hide_msg_sender: false,
            }],
            auth_witnesses: vec![AuthWitness {
                fields: vec![Fr::from(1u64)],
                ..Default::default()
            }],
            capsules: vec![],
            extra_hashed_args: vec![],
            fee_payer: Some(AztecAddress(Fr::from(99u64))),
        };

        let json = serde_json::to_string(&payload).expect("serialize payload");
        let decoded: ExecutionPayload = serde_json::from_str(&json).expect("deserialize payload");
        assert_eq!(decoded, payload);
    }
}
