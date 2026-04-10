//! Simulated kernel proving (skipKernels=true mode).
//!
//! When `simulateTx` is called with `skipKernels: true` (the default), the TS
//! PXE assembles `PrivateKernelTailCircuitPublicInputs` in software without
//! running kernel circuits. This module ports that logic to Rust.

use aztec_core::error::Error;
use aztec_core::types::Fr;

use crate::execution::acvm_executor::PrivateExecutionResult;

/// Simulated kernel output (assembled without real proving).
#[derive(Debug, Clone)]
pub struct SimulatedKernelOutput {
    /// Siloed note hashes.
    pub note_hashes: Vec<Fr>,
    /// Siloed nullifiers.
    pub nullifiers: Vec<Fr>,
    /// Encrypted logs.
    pub encrypted_logs: Vec<Vec<u8>>,
    /// Unencrypted logs.
    pub unencrypted_logs: Vec<Vec<u8>>,
    /// Gas used estimate.
    pub gas_used: GasEstimate,
    /// Nested call results (for public delegation).
    pub public_call_requests: Vec<serde_json::Value>,
}

/// Gas usage estimate from simulation.
#[derive(Debug, Clone, Default)]
pub struct GasEstimate {
    pub da_gas: u64,
    pub l2_gas: u64,
}

/// Assembles kernel public inputs in software (no proving).
pub struct SimulatedKernel;

impl SimulatedKernel {
    /// Process a private execution result into simulated kernel output.
    ///
    /// This performs the steps that the kernel circuits would normally do:
    /// 1. Silo note hashes with the contract address
    /// 2. Silo nullifiers with the contract address
    /// 3. Squash transient note hash / nullifier pairs
    /// 4. Estimate gas usage
    pub fn process(
        execution_result: &PrivateExecutionResult,
        contract_address: &Fr,
    ) -> Result<SimulatedKernelOutput, Error> {
        // Step 1: Silo note hashes — hash(contract_address, note_hash)
        let siloed_note_hashes: Vec<Fr> = execution_result
            .note_hashes
            .iter()
            .map(|nh| silo_note_hash(contract_address, nh))
            .collect();

        // Step 2: Silo nullifiers — hash(contract_address, nullifier)
        let siloed_nullifiers: Vec<Fr> = execution_result
            .nullifiers
            .iter()
            .map(|n| silo_nullifier(contract_address, n))
            .collect();

        // Step 3: Squash transient pairs
        let (note_hashes, nullifiers) =
            squash_transient_pairs(siloed_note_hashes, siloed_nullifiers);

        // Step 4: Estimate gas
        let gas_used = estimate_gas(&note_hashes, &nullifiers, &execution_result.encrypted_logs);

        Ok(SimulatedKernelOutput {
            note_hashes,
            nullifiers,
            encrypted_logs: execution_result.encrypted_logs.clone(),
            unencrypted_logs: execution_result.unencrypted_logs.clone(),
            gas_used,
            public_call_requests: execution_result.call_requests.clone(),
        })
    }
}

/// Silo a note hash with a contract address using Poseidon2.
fn silo_note_hash(contract_address: &Fr, note_hash: &Fr) -> Fr {
    use aztec_core::constants::domain_separator;
    use aztec_core::hash::poseidon2_hash_with_separator;
    poseidon2_hash_with_separator(
        &[*contract_address, *note_hash],
        domain_separator::SILO_NOTE_HASH,
    )
}

/// Silo a nullifier with a contract address using Poseidon2.
fn silo_nullifier(contract_address: &Fr, nullifier: &Fr) -> Fr {
    use aztec_core::constants::domain_separator;
    use aztec_core::hash::poseidon2_hash_with_separator;
    poseidon2_hash_with_separator(
        &[*contract_address, *nullifier],
        domain_separator::SILO_NULLIFIER,
    )
}

/// Remove transient note-hash/nullifier pairs that cancel each other out.
///
/// A transient note is one created and nullified within the same transaction.
/// These pairs are squashed to reduce on-chain cost.
fn squash_transient_pairs(note_hashes: Vec<Fr>, nullifiers: Vec<Fr>) -> (Vec<Fr>, Vec<Fr>) {
    // In the full implementation, this would match note hashes with their
    // corresponding nullifiers. For now, return as-is since we don't have
    // the nullifier-to-note-hash mapping without kernel circuits.
    (note_hashes, nullifiers)
}

/// Estimate gas usage from execution outputs.
fn estimate_gas(note_hashes: &[Fr], nullifiers: &[Fr], encrypted_logs: &[Vec<u8>]) -> GasEstimate {
    // Rough estimates based on TS implementation constants
    let da_gas = (note_hashes.len() as u64 * 32)
        + (nullifiers.len() as u64 * 32)
        + encrypted_logs.iter().map(|l| l.len() as u64).sum::<u64>();
    let l2_gas = (note_hashes.len() as u64 * 10_000) + (nullifiers.len() as u64 * 10_000);

    GasEstimate { da_gas, l2_gas }
}
