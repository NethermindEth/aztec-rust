//! ACVM integration for executing Noir bytecode.
//!
//! This module provides the bridge between compiled Aztec contract artifacts
//! (ACIR bytecode) and the Noir ACVM (Abstract Circuit Virtual Machine).

use acir::brillig::{ForeignCallParam, ForeignCallResult};
use acir::circuit::Program;
use acir::native_types::{Witness, WitnessMap};
use acir::FieldElement;
use acvm::pwg::{ACVMStatus, ACVM};
use bn254_blackbox_solver::Bn254BlackBoxSolver;

use aztec_core::abi::ContractArtifact;
use aztec_core::error::Error;
use aztec_core::types::Fr;

use super::field_conversion::{fe_to_fr, fr_to_fe, witness_map_to_frs};

/// Raw ACVM execution output before structuring into kernel types.
///
/// This contains the solved witness and return values from a single ACVM run.
/// The oracle is responsible for collecting side effects (notes, nullifiers, logs)
/// into the proper `PrivateCallExecutionResult` structure.
#[derive(Debug, Clone)]
pub struct AcvmExecutionOutput {
    /// Return values from the function.
    pub return_values: Vec<Fr>,
    /// The full solved witness map (for kernel circuit input).
    pub witness: WitnessMap<FieldElement>,
    /// The ACIR bytecode used (for kernel proving).
    pub acir_bytecode: Vec<u8>,
}

/// Result of executing a utility (unconstrained) function.
#[derive(Debug, Clone)]
pub struct UtilityResult {
    /// Return values from the function.
    pub return_values: Vec<Fr>,
}

/// Trait for oracle callback during ACVM execution.
///
/// Using a trait instead of a closure avoids the lifetime issues
/// with async closures capturing mutable references.
#[async_trait::async_trait]
pub trait OracleCallback: Send {
    async fn handle_foreign_call(
        &mut self,
        function: &str,
        inputs: Vec<Vec<Fr>>,
    ) -> Result<Vec<Vec<Fr>>, Error>;
}

/// Executor for Noir ACIR/Brillig bytecode via the ACVM.
pub struct AcvmExecutor;

impl AcvmExecutor {
    /// Decode base64-encoded bytecode and deserialize into an ACIR Program.
    fn decode_program(bytecode_b64: &str) -> Result<Program<FieldElement>, Error> {
        let bytecode_bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, bytecode_b64)
                .map_err(|e| Error::InvalidData(format!("base64 decode error: {e}")))?;

        Program::deserialize_program(&bytecode_bytes)
            .map_err(|e| Error::InvalidData(format!("ACIR deserialize error: {e}")))
    }

    /// Build the initial witness map from field elements.
    fn build_initial_witness(args: &[Fr]) -> WitnessMap<FieldElement> {
        let mut witness_map = WitnessMap::default();
        for (i, arg) in args.iter().enumerate() {
            // Match upstream `toACVMWitness(0, fields)`: witness indices start at 0.
            witness_map.insert(Witness(i as u32), fr_to_fe(arg));
        }
        witness_map
    }

    /// Convert ACVM foreign call inputs to Vec<Vec<Fr>>.
    fn convert_fc_inputs(inputs: &[ForeignCallParam<FieldElement>]) -> Vec<Vec<Fr>> {
        inputs
            .iter()
            .map(|param| match param {
                ForeignCallParam::Single(fe) => vec![fe_to_fr(fe)],
                ForeignCallParam::Array(fes) => fes.iter().map(fe_to_fr).collect(),
            })
            .collect()
    }

    /// Convert Vec<Vec<Fr>> oracle result to ForeignCallResult.
    ///
    /// Some Noir oracle interfaces return fixed-size arrays of length 1. Those must
    /// remain arrays, not be collapsed into scalars, or Brillig deserialization fails.
    fn convert_fc_result(function: &str, result: Vec<Vec<Fr>>) -> ForeignCallResult<FieldElement> {
        let force_array_indexes: &[usize] = match function {
            "utilityLoadCapsule" | "loadCapsule" | "getCapsule" => &[1],
            _ => &[],
        };
        let values: Vec<ForeignCallParam<FieldElement>> = result
            .into_iter()
            .enumerate()
            .map(|(index, frs)| {
                if frs.len() == 1 && !force_array_indexes.contains(&index) {
                    ForeignCallParam::Single(fr_to_fe(&frs[0]))
                } else {
                    ForeignCallParam::Array(frs.iter().map(fr_to_fe).collect())
                }
            })
            .collect();
        ForeignCallResult { values }
    }

    /// Execute a constrained (private) function from a contract artifact.
    ///
    /// Returns the raw ACVM output. The caller (oracle) is responsible for
    /// assembling side effects into a `PrivateCallExecutionResult`.
    pub async fn execute_private(
        artifact: &ContractArtifact,
        function_name: &str,
        initial_witness_fields: &[Fr],
        oracle: &mut dyn OracleCallback,
    ) -> Result<AcvmExecutionOutput, Error> {
        let function = artifact.find_function(function_name)?;

        let bytecode_b64 = function.bytecode.as_ref().ok_or_else(|| {
            Error::InvalidData(format!(
                "function '{}' in '{}' has no bytecode",
                function_name, artifact.name
            ))
        })?;

        let program = Self::decode_program(bytecode_b64)?;
        let initial_witness = Self::build_initial_witness(initial_witness_fields);

        let main_circuit = program
            .functions
            .first()
            .ok_or_else(|| Error::InvalidData("program has no circuits".to_string()))?;

        let backend = Bn254BlackBoxSolver;
        let empty_assertions = [];
        let mut acvm = ACVM::new(
            &backend,
            &main_circuit.opcodes,
            initial_witness,
            &program.unconstrained_functions,
            &empty_assertions,
        );

        // Solve loop with oracle dispatch
        loop {
            let status = acvm.solve();
            match status {
                ACVMStatus::Solved => break,
                ACVMStatus::InProgress => continue,
                ACVMStatus::RequiresForeignCall(foreign_call) => {
                    let inputs = Self::convert_fc_inputs(&foreign_call.inputs);
                    let result = oracle
                        .handle_foreign_call(&foreign_call.function, inputs)
                        .await?;
                    acvm.resolve_pending_foreign_call(Self::convert_fc_result(
                        &foreign_call.function,
                        result,
                    ));
                }
                ACVMStatus::RequiresAcirCall(acir_call) => {
                    let called_circuit_idx = acir_call.id.0 as usize;
                    if called_circuit_idx >= program.functions.len() {
                        return Err(Error::InvalidData(format!(
                            "ACIR call references circuit {} but program only has {}",
                            called_circuit_idx,
                            program.functions.len()
                        )));
                    }
                    let called_circuit = &program.functions[called_circuit_idx];
                    let sub_witness = acir_call.initial_witness;
                    let mut sub_acvm = ACVM::new(
                        &backend,
                        &called_circuit.opcodes,
                        sub_witness,
                        &program.unconstrained_functions,
                        &empty_assertions,
                    );
                    loop {
                        let sub_status = sub_acvm.solve();
                        match sub_status {
                            ACVMStatus::Solved => break,
                            ACVMStatus::InProgress => continue,
                            ACVMStatus::RequiresForeignCall(fc) => {
                                let inputs = Self::convert_fc_inputs(&fc.inputs);
                                let result =
                                    oracle.handle_foreign_call(&fc.function, inputs).await?;
                                sub_acvm.resolve_pending_foreign_call(Self::convert_fc_result(
                                    &fc.function,
                                    result,
                                ));
                            }
                            ACVMStatus::RequiresAcirCall(_) => {
                                return Err(Error::InvalidData(
                                    "nested ACIR calls deeper than 2 levels not supported".into(),
                                ));
                            }
                            ACVMStatus::Failure(err) => {
                                return Err(Error::InvalidData(format!(
                                    "sub-circuit execution failed: {err}"
                                )));
                            }
                        }
                    }
                    let sub_witness_map = sub_acvm.finalize();
                    let return_values: Vec<FieldElement> = called_circuit
                        .return_values
                        .0
                        .iter()
                        .filter_map(|w| sub_witness_map.get(w).copied())
                        .collect();
                    acvm.resolve_pending_acir_call(return_values);
                }
                ACVMStatus::Failure(err) => {
                    return Err(Error::InvalidData(format!(
                        "private function '{}' execution failed: {err}",
                        function_name
                    )));
                }
            }
        }

        let witness = acvm.finalize();
        let return_values = witness_map_to_frs(&witness, &main_circuit.return_values.0);

        // Capture the bytecode for kernel proving
        let acir_bytecode = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            function.bytecode.as_deref().unwrap_or(""),
        )
        .unwrap_or_default();

        Ok(AcvmExecutionOutput {
            return_values,
            witness,
            acir_bytecode,
        })
    }

    /// Execute a utility (unconstrained/Brillig) function.
    pub async fn execute_utility(
        artifact: &ContractArtifact,
        function_name: &str,
        args: &[Fr],
        oracle: &mut dyn OracleCallback,
    ) -> Result<UtilityResult, Error> {
        let function = artifact.find_function(function_name)?;

        let bytecode_b64 = function.bytecode.as_ref().ok_or_else(|| {
            Error::InvalidData(format!(
                "function '{}' in '{}' has no bytecode",
                function_name, artifact.name
            ))
        })?;

        let program = Self::decode_program(bytecode_b64)?;
        let initial_witness = Self::build_initial_witness(args);

        let main_circuit = program
            .functions
            .first()
            .ok_or_else(|| Error::InvalidData("program has no circuits".to_string()))?;

        let backend = Bn254BlackBoxSolver;
        let empty_assertions = [];
        let mut acvm = ACVM::new(
            &backend,
            &main_circuit.opcodes,
            initial_witness,
            &program.unconstrained_functions,
            &empty_assertions,
        );

        loop {
            let status = acvm.solve();
            match status {
                ACVMStatus::Solved => break,
                ACVMStatus::InProgress => continue,
                ACVMStatus::RequiresForeignCall(foreign_call) => {
                    let inputs = Self::convert_fc_inputs(&foreign_call.inputs);
                    let result = oracle
                        .handle_foreign_call(&foreign_call.function, inputs)
                        .await?;
                    acvm.resolve_pending_foreign_call(Self::convert_fc_result(
                        &foreign_call.function,
                        result,
                    ));
                }
                ACVMStatus::RequiresAcirCall(_) => {
                    return Err(Error::InvalidData(
                        "utility functions should not make ACIR calls".into(),
                    ));
                }
                ACVMStatus::Failure(err) => {
                    return Err(Error::InvalidData(format!(
                        "utility function '{}' execution failed: {err}",
                        function_name
                    )));
                }
            }
        }

        let witness = acvm.finalize();
        let return_values = witness_map_to_frs(&witness, &main_circuit.return_values.0);

        Ok(UtilityResult { return_values })
    }
}
