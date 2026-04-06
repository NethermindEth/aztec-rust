use serde::{Deserialize, Serialize};

use crate::abi::{AbiValue, FunctionSelector, FunctionType};
use crate::types::{AztecAddress, Fr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TxHash(pub [u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TxStatus {
    Dropped,
    Pending,
    Proposed,
    Checkpointed,
    Proven,
    Finalized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxExecutionResult {
    Success,
    AppLogicReverted,
    TeardownReverted,
    BothReverted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxReceipt {
    pub tx_hash: TxHash,
    pub status: TxStatus,
    pub execution_result: Option<TxExecutionResult>,
    pub error: Option<String>,
    pub transaction_fee: Option<u128>,
    pub block_hash: Option<[u8; 32]>,
    pub block_number: Option<u64>,
    pub epoch_number: Option<u64>,
}

impl TxReceipt {
    pub const fn is_mined(&self) -> bool {
        matches!(
            self.status,
            TxStatus::Proposed | TxStatus::Checkpointed | TxStatus::Proven | TxStatus::Finalized
        )
    }

    pub fn is_pending(&self) -> bool {
        self.status == TxStatus::Pending
    }

    pub fn is_dropped(&self) -> bool {
        self.status == TxStatus::Dropped
    }

    pub fn has_execution_succeeded(&self) -> bool {
        self.execution_result == Some(TxExecutionResult::Success)
    }

    pub fn has_execution_reverted(&self) -> bool {
        self.execution_result.is_some() && !self.has_execution_succeeded()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionCall {
    pub to: AztecAddress,
    pub selector: FunctionSelector,
    pub args: Vec<AbiValue>,
    pub function_type: FunctionType,
    pub is_static: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuthWitness {
    #[serde(default)]
    pub fields: Vec<Fr>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Capsule {
    #[serde(default)]
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HashedValues {
    #[serde(default)]
    pub values: Vec<Fr>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExecutionPayload {
    #[serde(default)]
    pub calls: Vec<FunctionCall>,
    #[serde(default)]
    pub auth_witnesses: Vec<AuthWitness>,
    #[serde(default)]
    pub capsules: Vec<Capsule>,
    #[serde(default)]
    pub extra_hashed_args: Vec<HashedValues>,
    pub fee_payer: Option<AztecAddress>,
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn tx_status_roundtrip() {
        let statuses = [
            TxStatus::Dropped,
            TxStatus::Pending,
            TxStatus::Proposed,
            TxStatus::Checkpointed,
            TxStatus::Proven,
            TxStatus::Finalized,
        ];

        for status in statuses {
            let json = match serde_json::to_string(&status) {
                Ok(json) => json,
                Err(err) => panic!("serializing TxStatus should succeed: {err}"),
            };
            let decoded: TxStatus = match serde_json::from_str(&json) {
                Ok(decoded) => decoded,
                Err(err) => panic!("deserializing TxStatus should succeed: {err}"),
            };
            assert_eq!(decoded, status);
        }
    }

    #[test]
    fn receipt_helpers_work() {
        let receipt = TxReceipt {
            tx_hash: TxHash([0u8; 32]),
            status: TxStatus::Checkpointed,
            execution_result: Some(TxExecutionResult::Success),
            error: None,
            transaction_fee: None,
            block_hash: None,
            block_number: Some(1),
            epoch_number: Some(1),
        };

        assert!(receipt.is_mined());
        assert!(!receipt.is_pending());
        assert!(!receipt.is_dropped());
        assert!(receipt.has_execution_succeeded());
        assert!(!receipt.has_execution_reverted());
    }

    #[test]
    fn payload_serializes() {
        let payload = ExecutionPayload::default();
        let json = match serde_json::to_string(&payload) {
            Ok(json) => json,
            Err(err) => panic!("serializing ExecutionPayload should succeed: {err}"),
        };
        assert!(json.contains("\"calls\":[]"));
    }
}
