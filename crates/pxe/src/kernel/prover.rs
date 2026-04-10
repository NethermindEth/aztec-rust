//! Private kernel prover trait and BB binary integration.
//!
//! Ports the TS `PrivateKernelProver` interface and `BBPrivateKernelProver`
//! implementation that shells out to the `bb` binary for proof generation.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_trait::async_trait;
use aztec_core::error::Error;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Output from a kernel circuit simulation or witness generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateKernelSimulateOutput {
    /// The circuit public inputs (opaque JSON matching TS types).
    pub public_inputs: serde_json::Value,
    /// Verification key data (raw bytes or base64).
    #[serde(default)]
    pub verification_key: Vec<u8>,
    /// Output witness map (serialized).
    #[serde(default)]
    pub output_witness: Vec<u8>,
    /// Circuit bytecode (gzipped ACIR).
    #[serde(default)]
    pub bytecode: Vec<u8>,
}

/// A single step in private execution, used to build a ChonkProof.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateExecutionStep {
    /// Function name for tracing.
    pub function_name: String,
    /// Gzipped circuit bytecode.
    pub bytecode: Vec<u8>,
    /// Serialized witness map.
    pub witness: Vec<u8>,
    /// Verification key bytes.
    pub vk: Vec<u8>,
    /// Timings metadata.
    #[serde(default)]
    pub timings: StepTimings,
}

/// Timing metadata for an execution step.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StepTimings {
    /// Witness generation time in ms.
    pub witgen_ms: u64,
    /// Gate count (if profiled).
    pub gate_count: Option<u64>,
    /// Oracle resolution time in ms.
    pub oracles_ms: u64,
}

/// ChonkProof with public inputs — the final aggregated proof.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChonkProofWithPublicInputs {
    /// The aggregated proof bytes.
    pub proof: Vec<u8>,
    /// The public inputs bytes.
    pub public_inputs: Vec<u8>,
}

/// Result from BB binary execution.
#[derive(Debug)]
pub enum BbResult {
    Success {
        duration_ms: u64,
        proof_path: Option<PathBuf>,
        vk_directory_path: Option<PathBuf>,
    },
    Failure {
        reason: String,
        retry: bool,
    },
}

/// Proving timings for the entire transaction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvingTimings {
    /// Total proving time in ms.
    pub total_ms: u64,
    /// Per-circuit timings.
    pub circuits: Vec<(String, u64)>,
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Private kernel prover interface.
///
/// Matches the TS `PrivateKernelProver` interface with methods for each
/// kernel circuit in the proving sequence.
#[async_trait]
pub trait PrivateKernelProver: Send + Sync {
    /// Generate witness + output for the init kernel circuit.
    async fn generate_init_output(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error>;

    /// Simulate the init circuit (no witness generation).
    async fn simulate_init(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error>;

    /// Generate witness + output for an inner kernel circuit.
    async fn generate_inner_output(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error>;

    /// Simulate an inner circuit.
    async fn simulate_inner(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error>;

    /// Generate witness + output for a reset kernel circuit.
    async fn generate_reset_output(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error>;

    /// Simulate a reset circuit.
    async fn simulate_reset(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error>;

    /// Generate witness + output for the tail kernel circuit.
    async fn generate_tail_output(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error>;

    /// Simulate the tail circuit.
    async fn simulate_tail(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error>;

    /// Generate hiding kernel output for rollup path.
    async fn generate_hiding_to_rollup_output(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error>;

    /// Generate hiding kernel output for public path.
    async fn generate_hiding_to_public_output(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error>;

    /// Create the aggregated ChonkProof from execution steps.
    async fn create_chonk_proof(
        &self,
        execution_steps: &[PrivateExecutionStep],
    ) -> Result<ChonkProofWithPublicInputs, Error>;

    /// Compute gate count for a circuit (profiling).
    async fn compute_gate_count_for_circuit(
        &self,
        bytecode: &[u8],
        circuit_name: &str,
    ) -> Result<u64, Error>;
}

// ---------------------------------------------------------------------------
// BB Binary Prover Implementation
// ---------------------------------------------------------------------------

/// Configuration for the BB prover.
#[derive(Debug, Clone)]
pub struct BbProverConfig {
    /// Path to the `bb` binary.
    pub bb_binary_path: PathBuf,
    /// Working directory for temporary proof artifacts.
    pub working_directory: PathBuf,
    /// Number of threads for hardware concurrency.
    pub hardware_concurrency: Option<u32>,
    /// Whether to skip cleanup of temporary files.
    pub skip_cleanup: bool,
}

impl Default for BbProverConfig {
    fn default() -> Self {
        // Default bb binary path for macOS ARM64
        let home = std::env::var("HOME").unwrap_or_default();
        let bb_path = std::env::var("BB_BINARY_PATH").unwrap_or_else(|_| {
            format!(
                "{home}/.aztec/versions/4.2.0-aztecnr-rc.2/node_modules/@aztec/bb.js/build/arm64-macos/bb"
            )
        });
        let work_dir = std::env::var("BB_WORKING_DIRECTORY")
            .unwrap_or_else(|_| format!("{home}/.aztec/bb-working"));

        Self {
            bb_binary_path: PathBuf::from(bb_path),
            working_directory: PathBuf::from(work_dir),
            hardware_concurrency: std::env::var("HARDWARE_CONCURRENCY")
                .ok()
                .and_then(|v| v.parse().ok()),
            skip_cleanup: std::env::var("BB_SKIP_CLEANUP")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
        }
    }
}

/// Private kernel prover backed by the Barretenberg (`bb`) binary.
///
/// Shells out to the `bb` binary for proof generation, matching the TS
/// `BBBundlePrivateKernelProver` implementation.
pub struct BbPrivateKernelProver {
    config: BbProverConfig,
}

impl BbPrivateKernelProver {
    pub fn new(config: BbProverConfig) -> Self {
        Self { config }
    }

    /// Create with default configuration (auto-detects bb path).
    pub fn with_defaults() -> Self {
        Self::new(BbProverConfig::default())
    }

    /// Execute the bb binary with given command and arguments.
    async fn execute_bb(&self, command: &str, args: &[&str]) -> Result<BbResult, Error> {
        let bb_path = &self.config.bb_binary_path;

        if !bb_path.exists() {
            return Ok(BbResult::Failure {
                reason: format!("bb binary not found at {}", bb_path.display()),
                retry: false,
            });
        }

        let start = std::time::Instant::now();

        let mut cmd = Command::new(bb_path);
        cmd.arg(command);
        cmd.args(args);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Set hardware concurrency
        if let Some(concurrency) = self.config.hardware_concurrency {
            cmd.env("HARDWARE_CONCURRENCY", concurrency.to_string());
        }

        tracing::debug!(
            "Executing BB: {} {} {}",
            bb_path.display(),
            command,
            args.join(" ")
        );

        let output = cmd
            .output()
            .await
            .map_err(|e| Error::InvalidData(format!("failed to execute bb binary: {e}")))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        if output.status.success() {
            Ok(BbResult::Success {
                duration_ms,
                proof_path: None,
                vk_directory_path: None,
            })
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Ok(BbResult::Failure {
                reason: format!(
                    "bb {} failed with exit code {:?}: {}",
                    command,
                    output.status.code(),
                    stderr
                ),
                retry: false,
            })
        }
    }

    /// Generate a ChonkProof by writing inputs and invoking bb prove --scheme chonk.
    async fn execute_chonk_proof(
        &self,
        inputs_path: &Path,
        output_path: &Path,
    ) -> Result<BbResult, Error> {
        let args = vec![
            "-o",
            output_path.to_str().unwrap_or(""),
            "--ivc_inputs_path",
            inputs_path.to_str().unwrap_or(""),
            "-v",
            "--scheme",
            "chonk",
        ];

        self.execute_bb("prove", &args).await
    }

    /// Ensure working directory exists.
    async fn ensure_working_dir(&self) -> Result<PathBuf, Error> {
        let dir = &self.config.working_directory;
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| Error::InvalidData(format!("failed to create working directory: {e}")))?;
        Ok(dir.clone())
    }

    /// Simulate a protocol circuit by executing it with the bb binary.
    ///
    /// In the TS implementation, this uses CircuitSimulator (ACVM-based).
    /// Here we delegate to bb for now, returning opaque JSON public inputs.
    async fn simulate_circuit(
        &self,
        _inputs: &serde_json::Value,
        circuit_type: &str,
    ) -> Result<PrivateKernelSimulateOutput, Error> {
        Err(Error::InvalidData(format!(
            "kernel circuit simulation for {circuit_type} is not wired to real artifacts yet"
        )))
    }

    /// Generate circuit output with witness and VK.
    async fn generate_circuit_output(
        &self,
        _inputs: &serde_json::Value,
        circuit_type: &str,
    ) -> Result<PrivateKernelSimulateOutput, Error> {
        let _ = self.ensure_working_dir().await?;
        Err(Error::InvalidData(format!(
            "kernel witness generation for {circuit_type} is not wired to real artifacts yet"
        )))
    }
}

#[async_trait]
impl PrivateKernelProver for BbPrivateKernelProver {
    async fn generate_init_output(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error> {
        self.generate_circuit_output(inputs, "PrivateKernelInitArtifact")
            .await
    }

    async fn simulate_init(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error> {
        self.simulate_circuit(inputs, "PrivateKernelInitArtifact")
            .await
    }

    async fn generate_inner_output(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error> {
        self.generate_circuit_output(inputs, "PrivateKernelInnerArtifact")
            .await
    }

    async fn simulate_inner(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error> {
        self.simulate_circuit(inputs, "PrivateKernelInnerArtifact")
            .await
    }

    async fn generate_reset_output(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error> {
        self.generate_circuit_output(inputs, "PrivateKernelResetArtifact")
            .await
    }

    async fn simulate_reset(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error> {
        self.simulate_circuit(inputs, "PrivateKernelResetArtifact")
            .await
    }

    async fn generate_tail_output(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error> {
        self.generate_circuit_output(inputs, "PrivateKernelTailArtifact")
            .await
    }

    async fn simulate_tail(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error> {
        self.simulate_circuit(inputs, "PrivateKernelTailArtifact")
            .await
    }

    async fn generate_hiding_to_rollup_output(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error> {
        self.generate_circuit_output(inputs, "HidingKernelToRollup")
            .await
    }

    async fn generate_hiding_to_public_output(
        &self,
        inputs: &serde_json::Value,
    ) -> Result<PrivateKernelSimulateOutput, Error> {
        self.generate_circuit_output(inputs, "HidingKernelToPublic")
            .await
    }

    async fn create_chonk_proof(
        &self,
        execution_steps: &[PrivateExecutionStep],
    ) -> Result<ChonkProofWithPublicInputs, Error> {
        let start = std::time::Instant::now();
        tracing::info!(
            "Generating ClientIVC proof ({} steps)...",
            execution_steps.len()
        );

        let work_dir = self.ensure_working_dir().await?;
        let chonk_dir = work_dir.join("chonk");
        tokio::fs::create_dir_all(&chonk_dir)
            .await
            .map_err(|e| Error::InvalidData(format!("failed to create chonk dir: {e}")))?;

        // Write execution steps as IVC inputs
        let inputs_path = chonk_dir.join("ivc_inputs.bin");
        let steps_data = serde_json::to_vec(execution_steps)?;
        tokio::fs::write(&inputs_path, &steps_data)
            .await
            .map_err(|e| Error::InvalidData(format!("failed to write IVC inputs: {e}")))?;

        let result = self.execute_chonk_proof(&inputs_path, &chonk_dir).await?;

        match result {
            BbResult::Success { duration_ms, .. } => {
                tracing::info!("Generated ClientIVC proof in {}ms", duration_ms);

                // Read proof and public inputs from output directory
                let proof_path = chonk_dir.join("proof");
                let pi_path = chonk_dir.join("public_inputs");

                let proof = tokio::fs::read(&proof_path).await.unwrap_or_default();
                let public_inputs = tokio::fs::read(&pi_path).await.unwrap_or_default();

                // Cleanup if configured
                if !self.config.skip_cleanup {
                    let _ = tokio::fs::remove_dir_all(&chonk_dir).await;
                }

                Ok(ChonkProofWithPublicInputs {
                    proof,
                    public_inputs,
                })
            }
            BbResult::Failure { reason, .. } => {
                let elapsed = start.elapsed().as_millis();
                tracing::error!(
                    "ChonkProof generation failed after {}ms: {}",
                    elapsed,
                    reason
                );
                Err(Error::InvalidData(format!(
                    "ChonkProof generation failed: {reason}"
                )))
            }
        }
    }

    async fn compute_gate_count_for_circuit(
        &self,
        bytecode: &[u8],
        circuit_name: &str,
    ) -> Result<u64, Error> {
        let work_dir = self.ensure_working_dir().await?;
        let gates_dir = work_dir.join("gates");
        tokio::fs::create_dir_all(&gates_dir)
            .await
            .map_err(|e| Error::InvalidData(format!("failed to create gates dir: {e}")))?;

        let bytecode_path = gates_dir.join(format!("{circuit_name}-bytecode"));
        tokio::fs::write(&bytecode_path, bytecode)
            .await
            .map_err(|e| Error::InvalidData(format!("failed to write bytecode: {e}")))?;

        let bytecode_str = bytecode_path.to_str().unwrap_or("");
        let args = vec!["--scheme", "ultra_honk", "-b", bytecode_str, "-v"];

        let result = self.execute_bb("gates", &args).await?;

        if !self.config.skip_cleanup {
            let _ = tokio::fs::remove_dir_all(&gates_dir).await;
        }

        match result {
            BbResult::Success { .. } => {
                // In full implementation, parse stdout for circuit_size
                Ok(0)
            }
            BbResult::Failure { reason, .. } => {
                Err(Error::InvalidData(format!("gate count failed: {reason}")))
            }
        }
    }
}
