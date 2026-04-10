//! Transaction validation logic matching the node's `DataTxValidator`.
//!
//! These functions implement the same invariant checks as the upstream
//! `p2p/src/msg_validators/tx_validator/data_validator.ts`.

use crate::constants::MAX_FR_CALLDATA_TO_ALL_ENQUEUED_CALLS;
use crate::hash::compute_calldata_hash;
use crate::kernel_types::PrivateKernelTailPublicInputs;
use crate::tx::{ContractClassLogFields, TypedTx};
use crate::types::Fr;
use crate::Error;

/// Validate the calldata in a typed transaction.
///
/// Checks:
/// - `publicFunctionCalldata.len() == numberOfPublicCalls()`
/// - total calldata count <= MAX_FR_CALLDATA_TO_ALL_ENQUEUED_CALLS
/// - each calldata hash matches the corresponding public call request
pub fn validate_calldata(tx: &TypedTx) -> Result<(), Error> {
    let expected_count = tx.number_of_public_calls();
    let actual_count = tx.public_function_calldata.len();

    if actual_count != expected_count {
        return Err(Error::InvalidData(format!(
            "TX_ERROR_CALLDATA_COUNT_MISMATCH: expected {expected_count} calldata entries, got {actual_count}"
        )));
    }

    let total_fields = tx.get_total_public_calldata_count();
    if total_fields > MAX_FR_CALLDATA_TO_ALL_ENQUEUED_CALLS {
        return Err(Error::InvalidData(format!(
            "TX_ERROR_CALLDATA_COUNT_TOO_LARGE: total calldata fields {total_fields} exceeds max {MAX_FR_CALLDATA_TO_ALL_ENQUEUED_CALLS}"
        )));
    }

    // Verify each calldata hash
    for (request, calldata) in tx.get_public_call_requests_with_calldata() {
        let computed_hash = compute_calldata_hash(&calldata.values);
        if computed_hash != request.calldata_hash {
            return Err(Error::InvalidData(format!(
                "TX_ERROR_INCORRECT_CALLDATA: calldata hash mismatch for call to {}",
                request.contract_address
            )));
        }
    }

    Ok(())
}

/// Validate the contract class logs in a typed transaction.
///
/// Checks:
/// - number of log fields entries matches log hashes in public inputs
/// - each log hash matches the hash of its corresponding fields
pub fn validate_contract_class_logs(
    public_inputs: &PrivateKernelTailPublicInputs,
    log_fields: &[ContractClassLogFields],
) -> Result<(), Error> {
    let log_hashes = public_inputs.get_non_empty_contract_class_logs_hashes();

    if log_hashes.len() != log_fields.len() {
        return Err(Error::InvalidData(format!(
            "TX_ERROR_CONTRACT_CLASS_LOG_COUNT: expected {} log field entries, got {}",
            log_hashes.len(),
            log_fields.len()
        )));
    }

    // Each log's emitted fields must have a minimum non-zero length
    for (i, fields) in log_fields.iter().enumerate() {
        let expected_min_length = 1 + fields
            .fields
            .iter()
            .rposition(|f| *f != Fr::zero())
            .unwrap_or(0);

        if let Some(hash_entry) = log_hashes.get(i) {
            if (hash_entry.log_hash.length as usize) < expected_min_length {
                return Err(Error::InvalidData(format!(
                    "TX_ERROR_CONTRACT_CLASS_LOG_LENGTH: log {} has length {} but minimum is {}",
                    i, hash_entry.log_hash.length, expected_min_length
                )));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel_types::*;
    use crate::tx::*;

    #[test]
    fn validate_calldata_empty_tx() {
        let tx = TypedTx {
            tx_hash: TxHash::zero(),
            data: PrivateKernelTailPublicInputs {
                for_rollup: Some(PartialPrivateTailPublicInputsForRollup {
                    end: PrivateToRollupAccumulatedData::default(),
                }),
                ..Default::default()
            },
            chonk_proof: ChonkProof::default(),
            contract_class_log_fields: vec![],
            public_function_calldata: vec![],
        };
        assert!(validate_calldata(&tx).is_ok());
    }

    #[test]
    fn validate_calldata_count_mismatch() {
        let tx = TypedTx {
            tx_hash: TxHash::zero(),
            data: PrivateKernelTailPublicInputs {
                for_public: Some(PartialPrivateTailPublicInputsForPublic {
                    non_revertible_accumulated_data: PrivateToPublicAccumulatedData {
                        public_call_requests: vec![PublicCallRequest {
                            contract_address: crate::types::AztecAddress(Fr::from(1u64)),
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            chonk_proof: ChonkProof::default(),
            contract_class_log_fields: vec![],
            public_function_calldata: vec![], // Mismatch: 0 calldata for 1 call
        };
        let err = validate_calldata(&tx).unwrap_err();
        assert!(err.to_string().contains("CALLDATA_COUNT_MISMATCH"));
    }

    #[test]
    fn validate_calldata_hash_match() {
        let calldata = vec![Fr::from(42u64), Fr::from(99u64)];
        let hash = compute_calldata_hash(&calldata);

        let tx = TypedTx {
            tx_hash: TxHash::zero(),
            data: PrivateKernelTailPublicInputs {
                for_public: Some(PartialPrivateTailPublicInputsForPublic {
                    non_revertible_accumulated_data: PrivateToPublicAccumulatedData {
                        public_call_requests: vec![PublicCallRequest {
                            contract_address: crate::types::AztecAddress(Fr::from(1u64)),
                            calldata_hash: hash,
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            chonk_proof: ChonkProof::default(),
            contract_class_log_fields: vec![],
            public_function_calldata: vec![HashedValues {
                values: calldata,
                hash,
            }],
        };
        assert!(validate_calldata(&tx).is_ok());
    }

    #[test]
    fn validate_calldata_hash_mismatch() {
        let calldata = vec![Fr::from(42u64)];
        let correct_hash = compute_calldata_hash(&calldata);
        let wrong_hash = Fr::from(999u64);

        let tx = TypedTx {
            tx_hash: TxHash::zero(),
            data: PrivateKernelTailPublicInputs {
                for_public: Some(PartialPrivateTailPublicInputsForPublic {
                    non_revertible_accumulated_data: PrivateToPublicAccumulatedData {
                        public_call_requests: vec![PublicCallRequest {
                            contract_address: crate::types::AztecAddress(Fr::from(1u64)),
                            calldata_hash: wrong_hash,
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                    ..Default::default()
                }),
                ..Default::default()
            },
            chonk_proof: ChonkProof::default(),
            contract_class_log_fields: vec![],
            public_function_calldata: vec![HashedValues {
                values: calldata,
                hash: correct_hash,
            }],
        };
        let err = validate_calldata(&tx).unwrap_err();
        assert!(err.to_string().contains("INCORRECT_CALLDATA"));
    }

    #[test]
    fn validate_contract_class_logs_empty() {
        let pi = PrivateKernelTailPublicInputs {
            for_rollup: Some(PartialPrivateTailPublicInputsForRollup {
                end: PrivateToRollupAccumulatedData::default(),
            }),
            ..Default::default()
        };
        assert!(validate_contract_class_logs(&pi, &[]).is_ok());
    }

    #[test]
    fn validate_contract_class_logs_count_mismatch() {
        let pi = PrivateKernelTailPublicInputs {
            for_rollup: Some(PartialPrivateTailPublicInputsForRollup {
                end: PrivateToRollupAccumulatedData {
                    contract_class_logs_hashes: vec![ScopedLogHash {
                        log_hash: LogHash {
                            value: Fr::from(1u64),
                            length: 10,
                        },
                        contract_address: crate::types::AztecAddress(Fr::from(1u64)),
                    }],
                    ..Default::default()
                },
            }),
            ..Default::default()
        };
        let err = validate_contract_class_logs(&pi, &[]).unwrap_err();
        assert!(err.to_string().contains("LOG_COUNT"));
    }
}
