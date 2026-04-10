//! ACVM integration for executing Noir bytecode.
//!
//! This module provides the bridge between compiled Aztec contract artifacts
//! (ACIR bytecode) and the Noir ACVM (Abstract Circuit Virtual Machine).
//!
//! **Phase 1 status:** Structured for future ACVM integration. The actual
//! `acvm` crate dependency will be added once the correct Noir version is
//! pinned to match the contract artifact compiler version.

use aztec_core::abi::ContractArtifact;
use aztec_core::error::Error;
use aztec_core::types::Fr;

/// Result of executing a private function.
#[derive(Debug, Clone)]
pub struct PrivateExecutionResult {
    /// Return values from the function.
    pub return_values: Vec<Fr>,
    /// Note hashes created during execution.
    pub note_hashes: Vec<Fr>,
    /// Nullifiers emitted during execution.
    pub nullifiers: Vec<Fr>,
    /// Encrypted log data emitted.
    pub encrypted_logs: Vec<Vec<u8>>,
    /// Unencrypted log data emitted.
    pub unencrypted_logs: Vec<Vec<u8>>,
    /// Nested call requests.
    pub call_requests: Vec<serde_json::Value>,
}

/// Result of executing a utility (unconstrained) function.
#[derive(Debug, Clone)]
pub struct UtilityResult {
    /// Return values from the function.
    pub return_values: Vec<Fr>,
}

/// Executor for Noir ACIR/Brillig bytecode.
///
/// In Phase 1 this serves as a structural placeholder. Once the `acvm` crate
/// is added as a dependency (pinned to the correct Noir version), the
/// `execute_private` and `execute_utility` methods will use the real ACVM.
pub struct AcvmExecutor;

impl AcvmExecutor {
    /// Execute a private function from a contract artifact.
    ///
    /// # Arguments
    /// * `artifact` - The contract artifact containing the function bytecode
    /// * `function_name` - Name of the function to execute
    /// * `args` - Function arguments as field elements
    /// * `oracle_callback` - Async callback for handling foreign calls
    ///
    /// # Phase 1 Status
    /// Returns a placeholder result. Real ACVM execution requires:
    /// 1. Pinning the `acvm` crate to the Noir version matching compiled artifacts
    /// 2. Deserializing the artifact's ACIR bytecode into `Program`
    /// 3. Running the ACVM solve loop with oracle callbacks
    pub async fn execute_private<F, Fut>(
        artifact: &ContractArtifact,
        function_name: &str,
        args: &[Fr],
        _oracle_callback: F,
    ) -> Result<PrivateExecutionResult, Error>
    where
        F: FnMut(&str, &[Vec<Fr>]) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<Vec<Fr>>, Error>>,
    {
        // Verify the function exists in the artifact
        let _function = artifact.find_function(function_name)?;

        let _ = args; // Will be used as initial witness

        // TODO(Phase 1): Integrate with the acvm crate.
        //
        // The execution loop will look like:
        // ```
        // let program = Program::deserialize(&function.bytecode)?;
        // let mut acvm = ACVM::new(&program, initial_witness);
        // loop {
        //     match acvm.solve() {
        //         Solved => break,
        //         RequiresForeignCall(req) => {
        //             let resp = oracle_callback(req.name, req.inputs).await?;
        //             acvm.resolve_pending_foreign_call(resp);
        //         }
        //         Failure(err) => return Err(err.into()),
        //     }
        // }
        // ```
        Err(Error::InvalidData(
            "ACVM execution not yet available — acvm crate integration pending (Phase 1)".into(),
        ))
    }

    /// Execute a utility (unconstrained/Brillig) function.
    pub async fn execute_utility<F, Fut>(
        artifact: &ContractArtifact,
        function_name: &str,
        args: &[Fr],
        _oracle_callback: F,
    ) -> Result<UtilityResult, Error>
    where
        F: FnMut(&str, &[Vec<Fr>]) -> Fut,
        Fut: std::future::Future<Output = Result<Vec<Vec<Fr>>, Error>>,
    {
        let _function = artifact.find_function(function_name)?;

        let _ = args;

        // TODO(Phase 1): Integrate with the acvm crate for Brillig execution.
        Err(Error::InvalidData(
            "ACVM utility execution not yet available — acvm crate integration pending (Phase 1)"
                .into(),
        ))
    }
}
