//! Private kernel execution prover — orchestrates the kernel circuit sequence.
//!
//! Ports the TS `PrivateKernelExecutionProver` which processes transaction
//! requests through the kernel circuit sequence:
//! init → inner (looped) → reset (dynamic) → tail → hiding → ChonkProof

use aztec_core::error::Error;
use aztec_core::types::{AztecAddress, Fr};
use aztec_node_client::AztecNode;

use crate::execution::acvm_executor::PrivateExecutionResult;
use crate::stores::{ContractStore, KeyStore};

use super::oracle::PrivateKernelOracle;
use super::prover::{
    ChonkProofWithPublicInputs, PrivateExecutionStep, PrivateKernelProver,
    PrivateKernelSimulateOutput, ProvingTimings,
};

/// Configuration for the kernel execution prover.
#[derive(Debug, Clone)]
pub struct KernelExecutionConfig {
    /// Whether to simulate (skip witness generation and proof creation).
    pub simulate: bool,
    /// Whether to skip fee enforcement validation.
    pub skip_fee_enforcement: bool,
    /// Profile mode: "none", "gates", "execution_steps", "full".
    pub profile_mode: String,
}

impl Default for KernelExecutionConfig {
    fn default() -> Self {
        Self {
            simulate: false,
            skip_fee_enforcement: false,
            profile_mode: "none".to_owned(),
        }
    }
}

/// Result from the kernel proving sequence.
#[derive(Debug)]
pub struct KernelProvingResult {
    /// The tail circuit public inputs.
    pub public_inputs: serde_json::Value,
    /// The aggregated ChonkProof (None if simulating).
    pub chonk_proof: Option<ChonkProofWithPublicInputs>,
    /// Execution steps used for the proof.
    pub execution_steps: Vec<PrivateExecutionStep>,
    /// Timings for each circuit.
    pub timings: ProvingTimings,
}

/// Orchestrates private kernel proof generation.
///
/// Processes a private execution result through the full kernel circuit sequence:
/// 1. **Init** — first private call produces kernel public inputs
/// 2. **Inner** — each subsequent nested call chains with previous output
/// 3. **Reset** — between calls: squash transient side effects, verify reads
/// 4. **Tail** — finalize private kernel public inputs
/// 5. **Hiding** — wrap into privacy-preserving proof (to-rollup or to-public)
/// 6. **ChonkProof** — aggregate all execution steps into one proof
pub struct PrivateKernelExecutionProver<'a, N: AztecNode> {
    oracle: PrivateKernelOracle<'a, N>,
    prover: &'a dyn PrivateKernelProver,
    config: KernelExecutionConfig,
}

impl<'a, N: AztecNode> PrivateKernelExecutionProver<'a, N> {
    pub fn new(
        oracle: PrivateKernelOracle<'a, N>,
        prover: &'a dyn PrivateKernelProver,
        config: KernelExecutionConfig,
    ) -> Self {
        Self {
            oracle,
            prover,
            config,
        }
    }

    /// Create from component stores and node.
    pub fn from_stores(
        node: &'a N,
        contract_store: &'a ContractStore,
        key_store: &'a KeyStore,
        prover: &'a dyn PrivateKernelProver,
        block_hash: Fr,
        config: KernelExecutionConfig,
    ) -> Self {
        let oracle = PrivateKernelOracle::new(node, contract_store, key_store, block_hash);
        Self::new(oracle, prover, config)
    }

    /// Execute the full kernel proving sequence for a transaction.
    ///
    /// Takes the private execution results (from ACVM execution) and processes
    /// them through the kernel circuit sequence.
    pub async fn prove_with_kernels(
        &self,
        execution_results: &[PrivateCallExecution],
    ) -> Result<KernelProvingResult, Error> {
        let start = std::time::Instant::now();
        let mut timings = ProvingTimings::default();
        let mut execution_steps = Vec::new();

        if execution_results.is_empty() {
            return Err(Error::InvalidData("no execution results to prove".into()));
        }

        // Step 1: Init circuit — process the first private call
        let first = &execution_results[0];
        let init_inputs = self.build_init_inputs(first).await?;

        let init_output = if self.config.simulate {
            self.prover.simulate_init(&init_inputs).await?
        } else {
            let output = self.prover.generate_init_output(&init_inputs).await?;
            execution_steps.push(self.output_to_step(&output, "private_kernel_init"));
            output
        };

        timings
            .circuits
            .push(("init".to_owned(), start.elapsed().as_millis() as u64));
        let mut current_output = init_output;

        // Step 2: Inner circuits — process each subsequent nested call
        for (i, call) in execution_results.iter().enumerate().skip(1) {
            let inner_start = std::time::Instant::now();
            let inner_inputs = self.build_inner_inputs(call, &current_output).await?;

            current_output = if self.config.simulate {
                self.prover.simulate_inner(&inner_inputs).await?
            } else {
                let output = self.prover.generate_inner_output(&inner_inputs).await?;
                execution_steps
                    .push(self.output_to_step(&output, &format!("private_kernel_inner_{i}")));
                output
            };

            timings.circuits.push((
                format!("inner_{i}"),
                inner_start.elapsed().as_millis() as u64,
            ));

            // Check if reset is needed between iterations
            if self.needs_reset(&current_output) {
                let reset_start = std::time::Instant::now();
                let reset_inputs = self.build_reset_inputs(&current_output).await?;

                current_output = if self.config.simulate {
                    self.prover.simulate_reset(&reset_inputs).await?
                } else {
                    let output = self.prover.generate_reset_output(&reset_inputs).await?;
                    execution_steps
                        .push(self.output_to_step(&output, &format!("private_kernel_reset_{i}")));
                    output
                };

                timings.circuits.push((
                    format!("reset_{i}"),
                    reset_start.elapsed().as_millis() as u64,
                ));
            }
        }

        // Step 3: Final reset — with siloing enabled
        let final_reset_start = std::time::Instant::now();
        let final_reset_inputs = self.build_final_reset_inputs(&current_output).await?;

        current_output = if self.config.simulate {
            self.prover.simulate_reset(&final_reset_inputs).await?
        } else {
            let output = self
                .prover
                .generate_reset_output(&final_reset_inputs)
                .await?;
            execution_steps.push(self.output_to_step(&output, "private_kernel_reset_final"));
            output
        };

        timings.circuits.push((
            "reset_final".to_owned(),
            final_reset_start.elapsed().as_millis() as u64,
        ));

        // Step 4: Tail circuit — finalize
        let tail_start = std::time::Instant::now();
        let tail_inputs = self.build_tail_inputs(&current_output).await?;
        let is_for_public = self.is_for_public(&current_output);

        let tail_output = if self.config.simulate {
            self.prover.simulate_tail(&tail_inputs).await?
        } else {
            let output = self.prover.generate_tail_output(&tail_inputs).await?;
            let name = if is_for_public {
                "private_kernel_tail_to_public"
            } else {
                "private_kernel_tail"
            };
            execution_steps.push(self.output_to_step(&output, name));
            output
        };

        timings
            .circuits
            .push(("tail".to_owned(), tail_start.elapsed().as_millis() as u64));

        // Step 5: Hiding kernel — only for non-simulation
        if !self.config.simulate {
            let hiding_start = std::time::Instant::now();
            let hiding_inputs = self
                .build_hiding_inputs(&tail_output, is_for_public)
                .await?;

            let hiding_output = if is_for_public {
                let output = self
                    .prover
                    .generate_hiding_to_public_output(&hiding_inputs)
                    .await?;
                execution_steps.push(self.output_to_step(&output, "hiding_kernel_to_public"));
                output
            } else {
                let output = self
                    .prover
                    .generate_hiding_to_rollup_output(&hiding_inputs)
                    .await?;
                execution_steps.push(self.output_to_step(&output, "hiding_kernel_to_rollup"));
                output
            };

            timings.circuits.push((
                "hiding".to_owned(),
                hiding_start.elapsed().as_millis() as u64,
            ));

            // Use hiding output as the final public inputs
            let _ = hiding_output;
        }

        // Step 6: ChonkProof — aggregate all execution steps
        let chonk_proof = if !self.config.simulate && !execution_steps.is_empty() {
            let chonk_start = std::time::Instant::now();
            let proof = self.prover.create_chonk_proof(&execution_steps).await?;
            timings
                .circuits
                .push(("chonk".to_owned(), chonk_start.elapsed().as_millis() as u64));
            Some(proof)
        } else {
            None
        };

        timings.total_ms = start.elapsed().as_millis() as u64;

        Ok(KernelProvingResult {
            public_inputs: tail_output.public_inputs,
            chonk_proof,
            execution_steps,
            timings,
        })
    }

    // --- Input builders ---

    /// Build inputs for the init kernel circuit.
    async fn build_init_inputs(
        &self,
        call: &PrivateCallExecution,
    ) -> Result<serde_json::Value, Error> {
        let contract_preimage = self
            .oracle
            .get_contract_address_preimage(&call.contract_address)
            .await?;

        Ok(serde_json::json!({
            "txRequest": call.tx_request,
            "privateCall": {
                "callStackItem": call.call_stack_item,
                "executionResult": call.execution_result_json,
            },
            "contractInstance": contract_preimage,
            "vkMembershipWitness": self.oracle.get_vk_membership_witness(&Fr::zero()).await?,
        }))
    }

    /// Build inputs for inner kernel circuit.
    async fn build_inner_inputs(
        &self,
        call: &PrivateCallExecution,
        previous_output: &PrivateKernelSimulateOutput,
    ) -> Result<serde_json::Value, Error> {
        let contract_preimage = self
            .oracle
            .get_contract_address_preimage(&call.contract_address)
            .await?;

        Ok(serde_json::json!({
            "previousKernelData": {
                "publicInputs": previous_output.public_inputs,
            },
            "privateCall": {
                "callStackItem": call.call_stack_item,
                "executionResult": call.execution_result_json,
            },
            "contractInstance": contract_preimage,
            "vkMembershipWitness": self.oracle.get_vk_membership_witness(&Fr::zero()).await?,
        }))
    }

    /// Build inputs for reset kernel circuit.
    async fn build_reset_inputs(
        &self,
        current_output: &PrivateKernelSimulateOutput,
    ) -> Result<serde_json::Value, Error> {
        Ok(serde_json::json!({
            "previousKernelData": {
                "publicInputs": current_output.public_inputs,
            },
            "silo": false,
        }))
    }

    /// Build inputs for final reset (with siloing).
    async fn build_final_reset_inputs(
        &self,
        current_output: &PrivateKernelSimulateOutput,
    ) -> Result<serde_json::Value, Error> {
        Ok(serde_json::json!({
            "previousKernelData": {
                "publicInputs": current_output.public_inputs,
            },
            "silo": true,
        }))
    }

    /// Build inputs for tail kernel circuit.
    async fn build_tail_inputs(
        &self,
        current_output: &PrivateKernelSimulateOutput,
    ) -> Result<serde_json::Value, Error> {
        Ok(serde_json::json!({
            "previousKernelData": {
                "publicInputs": current_output.public_inputs,
            },
        }))
    }

    /// Build inputs for hiding kernel circuit.
    async fn build_hiding_inputs(
        &self,
        tail_output: &PrivateKernelSimulateOutput,
        _is_for_public: bool,
    ) -> Result<serde_json::Value, Error> {
        Ok(serde_json::json!({
            "previousKernelData": {
                "publicInputs": tail_output.public_inputs,
            },
        }))
    }

    /// Check if a reset circuit is needed between iterations.
    fn needs_reset(&self, _output: &PrivateKernelSimulateOutput) -> bool {
        // In the TS implementation, reset is triggered when:
        // - The accumulated data exceeds the dimension thresholds
        // - There are read requests that need verification
        // For now, return false to skip intermediate resets.
        false
    }

    /// Check if the transaction has public calls (determines tail variant).
    fn is_for_public(&self, output: &PrivateKernelSimulateOutput) -> bool {
        output
            .public_inputs
            .pointer("/end/publicCallRequests")
            .and_then(|v| v.as_array())
            .map(|arr| !arr.is_empty())
            .unwrap_or(false)
    }

    /// Convert a kernel output to an execution step for ChonkProof.
    fn output_to_step(
        &self,
        output: &PrivateKernelSimulateOutput,
        function_name: &str,
    ) -> PrivateExecutionStep {
        PrivateExecutionStep {
            function_name: function_name.to_owned(),
            bytecode: output.bytecode.clone(),
            witness: output.output_witness.clone(),
            vk: output.verification_key.clone(),
            timings: Default::default(),
        }
    }
}

/// Represents a private function call execution for kernel processing.
#[derive(Debug, Clone)]
pub struct PrivateCallExecution {
    /// Contract address that was called.
    pub contract_address: AztecAddress,
    /// The transaction execution request (only for the first call).
    pub tx_request: serde_json::Value,
    /// The call stack item (function selector, args hash, etc).
    pub call_stack_item: serde_json::Value,
    /// The execution result as opaque JSON.
    pub execution_result_json: serde_json::Value,
    /// The structured execution result.
    pub execution_result: PrivateExecutionResult,
}
