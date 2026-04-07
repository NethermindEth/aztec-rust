use std::time::Duration;
use std::{fmt, vec};

use async_trait::async_trait;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use aztec_core::abi::{ContractArtifact, EventSelector};
use aztec_core::error::Error;
use aztec_core::tx::{AuthWitness, FunctionCall, TxHash};
use aztec_core::types::{AztecAddress, CompleteAddress, ContractInstanceWithAddress, Fr};
use aztec_rpc::RpcTransport;

// ---------------------------------------------------------------------------
// Supporting types — opaque wrappers
// ---------------------------------------------------------------------------

fn strip_0x(s: &str) -> &str {
    s.strip_prefix("0x").unwrap_or(s)
}

fn decode_hex_32(s: &str) -> Result<[u8; 32], Error> {
    let raw = strip_0x(s);
    if raw.len() > 64 {
        return Err(Error::InvalidData(
            "hex value too large: expected at most 32 bytes".to_owned(),
        ));
    }

    let padded = if raw.len() % 2 == 1 {
        format!("0{raw}")
    } else {
        raw.to_owned()
    };

    let decoded = hex::decode(padded).map_err(|e| Error::InvalidData(e.to_string()))?;
    if decoded.len() > 32 {
        return Err(Error::InvalidData(
            "hex value too large: expected at most 32 bytes".to_owned(),
        ));
    }

    let mut out = [0u8; 32];
    out[32 - decoded.len()..].copy_from_slice(&decoded);
    Ok(out)
}

fn encode_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

/// A synced block header from the PXE.
///
/// The full block header has many fields; kept as opaque JSON until
/// the complete schema is ported to `aztec-core`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    /// The full block header data.
    #[serde(flatten)]
    pub data: serde_json::Value,
}

/// A transaction execution request submitted to the PXE.
///
/// Contains encoded function calls, auth witnesses, gas settings, etc.
/// Kept as opaque JSON until the full schema is ported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxExecutionRequest {
    /// The request data.
    #[serde(flatten)]
    pub data: serde_json::Value,
}

/// Result of proving a transaction via the PXE.
///
/// Contains the proven transaction data ready for submission to the node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxProvingResult {
    /// The proven transaction data.
    #[serde(flatten)]
    pub data: serde_json::Value,
}

/// Result of simulating a transaction via the PXE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxSimulationResult {
    /// The simulation result data.
    #[serde(flatten)]
    pub data: serde_json::Value,
}

/// Result of profiling a transaction via the PXE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxProfileResult {
    /// The profiling result data.
    #[serde(flatten)]
    pub data: serde_json::Value,
}

/// A 32-byte L2 block hash.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct BlockHash(pub [u8; 32]);

impl BlockHash {
    /// Parse a block hash from a hex string.
    pub fn from_hex(value: &str) -> Result<Self, Error> {
        Ok(Self(decode_hex_32(value)?))
    }
}

impl fmt::Display for BlockHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&encode_hex(&self.0))
    }
}

impl fmt::Debug for BlockHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BlockHash({self})")
    }
}

impl Serialize for BlockHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for BlockHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

/// A globally unique log identifier used for event pagination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogId {
    /// The L2 block number containing the log.
    pub block_number: u64,
    /// The L2 block hash containing the log.
    pub block_hash: BlockHash,
    /// The transaction hash that emitted the log.
    pub tx_hash: TxHash,
    /// The transaction index within the block.
    pub tx_index: u64,
    /// The log index within the transaction.
    pub log_index: u64,
}

/// Profiling modes supported by the PXE.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileMode {
    /// Full profiling.
    #[serde(rename = "full")]
    Full,
    /// Execution-steps profiling.
    #[serde(rename = "execution-steps")]
    ExecutionSteps,
    /// Gates profiling.
    #[serde(rename = "gates")]
    Gates,
}

// ---------------------------------------------------------------------------
// Supporting types — concrete structs
// ---------------------------------------------------------------------------

/// Result of executing a utility (view) function via the PXE.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UtilityExecutionResult {
    /// Raw field return values from the utility function.
    pub result: Vec<Fr>,
    /// Optional simulation stats payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<serde_json::Value>,
}

/// Options for PXE transaction simulation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulateTxOpts {
    /// Whether to simulate the public phase as well.
    #[serde(default)]
    pub simulate_public: bool,
    /// Whether to skip transaction validation.
    #[serde(default)]
    pub skip_tx_validation: bool,
    /// Whether to skip fee enforcement during simulation.
    #[serde(default)]
    pub skip_fee_enforcement: bool,
    /// Simulation-time state overrides.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<serde_json::Value>,
    /// Note-discovery scopes.
    #[serde(default)]
    pub scopes: Vec<AztecAddress>,
}

/// Options for PXE transaction profiling.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileTxOpts {
    /// The profiling mode to use.
    pub profile_mode: ProfileMode,
    /// Whether to skip proof generation during profiling.
    #[serde(default = "default_skip_proof_generation")]
    pub skip_proof_generation: bool,
    /// Note-discovery scopes.
    #[serde(default)]
    pub scopes: Vec<AztecAddress>,
}

impl Default for ProfileTxOpts {
    fn default() -> Self {
        Self {
            profile_mode: ProfileMode::Full,
            skip_proof_generation: default_skip_proof_generation(),
            scopes: vec![],
        }
    }
}

/// Options for executing a utility function via the PXE.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteUtilityOpts {
    /// Authentication witnesses required for the call.
    #[serde(default)]
    pub authwits: Vec<AuthWitness>,
    /// Note-discovery scopes.
    #[serde(default)]
    pub scopes: Vec<AztecAddress>,
}

/// A packed private event retrieved from the PXE.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackedPrivateEvent {
    /// The packed event data as field elements.
    pub packed_event: Vec<Fr>,
    /// Hash of the transaction that emitted the event.
    pub tx_hash: TxHash,
    /// L2 block number containing the event.
    pub l2_block_number: u64,
    /// L2 block hash containing the event.
    pub l2_block_hash: BlockHash,
    /// Selector identifying the event type.
    pub event_selector: EventSelector,
}

/// Filter for querying private events from the PXE.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateEventFilter {
    /// Contract to filter events from.
    pub contract_address: AztecAddress,
    /// Filter by transaction hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<TxHash>,
    /// Start block (inclusive).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_block: Option<u64>,
    /// End block (inclusive).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_block: Option<u64>,
    /// Cursor for pagination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_log: Option<LogId>,
    /// Note-discovery scopes.
    #[serde(default)]
    pub scopes: Vec<AztecAddress>,
}

impl Default for PrivateEventFilter {
    fn default() -> Self {
        Self {
            contract_address: AztecAddress(Fr::zero()),
            tx_hash: None,
            from_block: None,
            to_block: None,
            after_log: None,
            scopes: vec![],
        }
    }
}

/// Request to register a contract with the PXE.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterContractRequest {
    /// The contract instance to register.
    pub instance: ContractInstanceWithAddress,
    /// Optional contract artifact (can be omitted if class is already registered).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<ContractArtifact>,
}

const fn default_skip_proof_generation() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Pxe trait
// ---------------------------------------------------------------------------

/// Interface for the Aztec Private eXecution Environment (PXE).
///
/// The PXE handles private state, transaction simulation, proving,
/// and account/contract registration. This trait abstracts over
/// different PXE backends (HTTP client, in-process, mock).
#[async_trait]
pub trait Pxe: Send + Sync {
    /// Get the synced block header.
    async fn get_synced_block_header(&self) -> Result<BlockHeader, Error>;

    /// Get a contract instance by its address.
    async fn get_contract_instance(
        &self,
        address: &AztecAddress,
    ) -> Result<Option<ContractInstanceWithAddress>, Error>;

    /// Get a contract artifact by its class ID.
    async fn get_contract_artifact(&self, id: &Fr) -> Result<Option<ContractArtifact>, Error>;

    /// Get all registered contract addresses.
    async fn get_contracts(&self) -> Result<Vec<AztecAddress>, Error>;

    /// Register an account with the PXE.
    async fn register_account(
        &self,
        secret_key: &Fr,
        partial_address: &Fr,
    ) -> Result<CompleteAddress, Error>;

    /// Get all registered account addresses.
    async fn get_registered_accounts(&self) -> Result<Vec<CompleteAddress>, Error>;

    /// Register a sender address for private log syncing.
    async fn register_sender(&self, sender: &AztecAddress) -> Result<AztecAddress, Error>;

    /// Get all registered sender addresses.
    async fn get_senders(&self) -> Result<Vec<AztecAddress>, Error>;

    /// Remove a registered sender.
    async fn remove_sender(&self, sender: &AztecAddress) -> Result<(), Error>;

    /// Register a contract class with the PXE (artifact only, no instance).
    async fn register_contract_class(&self, artifact: &ContractArtifact) -> Result<(), Error>;

    /// Register a contract instance (and optionally its artifact).
    async fn register_contract(&self, request: RegisterContractRequest) -> Result<(), Error>;

    /// Update a contract's artifact.
    async fn update_contract(
        &self,
        address: &AztecAddress,
        artifact: &ContractArtifact,
    ) -> Result<(), Error>;

    /// Simulate a transaction without sending it.
    async fn simulate_tx(
        &self,
        tx_request: &TxExecutionRequest,
        opts: SimulateTxOpts,
    ) -> Result<TxSimulationResult, Error>;

    /// Prove a transaction, producing a proven result ready for submission.
    async fn prove_tx(
        &self,
        tx_request: &TxExecutionRequest,
        scopes: Vec<AztecAddress>,
    ) -> Result<TxProvingResult, Error>;

    /// Profile a transaction for gas estimation and performance data.
    async fn profile_tx(
        &self,
        tx_request: &TxExecutionRequest,
        opts: ProfileTxOpts,
    ) -> Result<TxProfileResult, Error>;

    /// Execute a utility (view/unconstrained) function.
    async fn execute_utility(
        &self,
        call: &FunctionCall,
        opts: ExecuteUtilityOpts,
    ) -> Result<UtilityExecutionResult, Error>;

    /// Get private events matching a selector and filter.
    async fn get_private_events(
        &self,
        event_selector: &EventSelector,
        filter: PrivateEventFilter,
    ) -> Result<Vec<PackedPrivateEvent>, Error>;

    /// Stop the PXE instance.
    async fn stop(&self) -> Result<(), Error>;
}

// ---------------------------------------------------------------------------
// HTTP client
// ---------------------------------------------------------------------------

/// HTTP JSON-RPC backed PXE client.
pub struct HttpPxeClient {
    transport: RpcTransport,
}

impl HttpPxeClient {
    fn new(url: String, timeout: Duration) -> Self {
        Self {
            transport: RpcTransport::new(url, timeout),
        }
    }
}

#[async_trait]
impl Pxe for HttpPxeClient {
    async fn get_synced_block_header(&self) -> Result<BlockHeader, Error> {
        self.transport
            .call("pxe_getSyncedBlockHeader", serde_json::json!([]))
            .await
    }

    async fn get_contract_instance(
        &self,
        address: &AztecAddress,
    ) -> Result<Option<ContractInstanceWithAddress>, Error> {
        self.transport
            .call("pxe_getContractInstance", serde_json::json!([address]))
            .await
    }

    async fn get_contract_artifact(&self, id: &Fr) -> Result<Option<ContractArtifact>, Error> {
        self.transport
            .call("pxe_getContractArtifact", serde_json::json!([id]))
            .await
    }

    async fn get_contracts(&self) -> Result<Vec<AztecAddress>, Error> {
        self.transport
            .call("pxe_getContracts", serde_json::json!([]))
            .await
    }

    async fn register_account(
        &self,
        secret_key: &Fr,
        partial_address: &Fr,
    ) -> Result<CompleteAddress, Error> {
        self.transport
            .call(
                "pxe_registerAccount",
                serde_json::json!([secret_key, partial_address]),
            )
            .await
    }

    async fn get_registered_accounts(&self) -> Result<Vec<CompleteAddress>, Error> {
        self.transport
            .call("pxe_getRegisteredAccounts", serde_json::json!([]))
            .await
    }

    async fn register_sender(&self, sender: &AztecAddress) -> Result<AztecAddress, Error> {
        self.transport
            .call("pxe_registerSender", serde_json::json!([sender]))
            .await
    }

    async fn get_senders(&self) -> Result<Vec<AztecAddress>, Error> {
        self.transport
            .call("pxe_getSenders", serde_json::json!([]))
            .await
    }

    async fn remove_sender(&self, sender: &AztecAddress) -> Result<(), Error> {
        self.transport
            .call_void("pxe_removeSender", serde_json::json!([sender]))
            .await
    }

    async fn register_contract_class(&self, artifact: &ContractArtifact) -> Result<(), Error> {
        self.transport
            .call_void("pxe_registerContractClass", serde_json::json!([artifact]))
            .await
    }

    async fn register_contract(&self, request: RegisterContractRequest) -> Result<(), Error> {
        self.transport
            .call_void("pxe_registerContract", serde_json::json!([request]))
            .await
    }

    async fn update_contract(
        &self,
        address: &AztecAddress,
        artifact: &ContractArtifact,
    ) -> Result<(), Error> {
        self.transport
            .call_void("pxe_updateContract", serde_json::json!([address, artifact]))
            .await
    }

    async fn simulate_tx(
        &self,
        tx_request: &TxExecutionRequest,
        opts: SimulateTxOpts,
    ) -> Result<TxSimulationResult, Error> {
        self.transport
            .call("pxe_simulateTx", serde_json::json!([tx_request, opts]))
            .await
    }

    async fn prove_tx(
        &self,
        tx_request: &TxExecutionRequest,
        scopes: Vec<AztecAddress>,
    ) -> Result<TxProvingResult, Error> {
        self.transport
            .call("pxe_proveTx", serde_json::json!([tx_request, scopes]))
            .await
    }

    async fn profile_tx(
        &self,
        tx_request: &TxExecutionRequest,
        opts: ProfileTxOpts,
    ) -> Result<TxProfileResult, Error> {
        self.transport
            .call("pxe_profileTx", serde_json::json!([tx_request, opts]))
            .await
    }

    async fn execute_utility(
        &self,
        call: &FunctionCall,
        opts: ExecuteUtilityOpts,
    ) -> Result<UtilityExecutionResult, Error> {
        self.transport
            .call("pxe_executeUtility", serde_json::json!([call, opts]))
            .await
    }

    async fn get_private_events(
        &self,
        event_selector: &EventSelector,
        filter: PrivateEventFilter,
    ) -> Result<Vec<PackedPrivateEvent>, Error> {
        self.transport
            .call(
                "pxe_getPrivateEvents",
                serde_json::json!([event_selector, filter]),
            )
            .await
    }

    async fn stop(&self) -> Result<(), Error> {
        self.transport
            .call_void("pxe_stop", serde_json::json!([]))
            .await
    }
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/// Create an HTTP JSON-RPC backed PXE client.
///
/// Uses a default timeout of 30 seconds.
pub fn create_pxe_client(url: impl Into<String>) -> HttpPxeClient {
    HttpPxeClient::new(url.into(), Duration::from_secs(30))
}

// ---------------------------------------------------------------------------
// Polling helpers
// ---------------------------------------------------------------------------

/// Wait for the PXE to become ready by retrying `get_synced_block_header`.
///
/// Uses a default timeout of 120 seconds with a 1 second polling interval.
/// Returns the [`BlockHeader`] on success, or a timeout error.
pub async fn wait_for_pxe(pxe: &(impl Pxe + ?Sized)) -> Result<BlockHeader, Error> {
    wait_for_pxe_opts(pxe, Duration::from_secs(120), Duration::from_secs(1)).await
}

async fn wait_for_pxe_opts(
    pxe: &(impl Pxe + ?Sized),
    timeout: Duration,
    interval: Duration,
) -> Result<BlockHeader, Error> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match pxe.get_synced_block_header().await {
            Ok(header) => return Ok(header),
            Err(_) if tokio::time::Instant::now() + interval < deadline => {
                tokio::time::sleep(interval).await;
            }
            Err(e) => {
                return Err(Error::Timeout(format!(
                    "PXE not ready after {timeout:?}: {e}"
                )));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use aztec_core::abi::{FunctionSelector, FunctionType};
    use aztec_core::types::{ContractInstance, PublicKeys};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    // -- Serde roundtrip tests --

    #[test]
    fn block_header_roundtrip() {
        let json_str = r#"{"globalVariables":{"blockNumber":42},"contentCommitment":"0x01"}"#;
        let header: BlockHeader = serde_json::from_str(json_str).unwrap();
        let reserialized = serde_json::to_string(&header).unwrap();
        let decoded: BlockHeader = serde_json::from_str(&reserialized).unwrap();
        assert_eq!(decoded.data["globalVariables"]["blockNumber"], 42);
    }

    #[test]
    fn tx_execution_request_roundtrip() {
        let json_str = r#"{"origin":"0x0000000000000000000000000000000000000000000000000000000000000001","functionSelector":"0xaabbccdd"}"#;
        let req: TxExecutionRequest = serde_json::from_str(json_str).unwrap();
        let reserialized = serde_json::to_string(&req).unwrap();
        let decoded: TxExecutionRequest = serde_json::from_str(&reserialized).unwrap();
        assert_eq!(decoded.data["functionSelector"], "0xaabbccdd");
    }

    #[test]
    fn tx_proving_result_roundtrip() {
        let json_str = r#"{"proof":"0xdeadbeef","publicInputs":[1,2,3]}"#;
        let result: TxProvingResult = serde_json::from_str(json_str).unwrap();
        let reserialized = serde_json::to_string(&result).unwrap();
        let decoded: TxProvingResult = serde_json::from_str(&reserialized).unwrap();
        assert_eq!(decoded.data["proof"], "0xdeadbeef");
    }

    #[test]
    fn tx_simulation_result_roundtrip() {
        let json_str = r#"{"returnValues":[42],"gasUsed":{"daGas":100,"l2Gas":200}}"#;
        let result: TxSimulationResult = serde_json::from_str(json_str).unwrap();
        let reserialized = serde_json::to_string(&result).unwrap();
        let decoded: TxSimulationResult = serde_json::from_str(&reserialized).unwrap();
        assert_eq!(decoded.data["gasUsed"]["l2Gas"], 200);
    }

    #[test]
    fn tx_profile_result_roundtrip() {
        let json_str = r#"{"gateCounts":[10,20],"executionSteps":5}"#;
        let result: TxProfileResult = serde_json::from_str(json_str).unwrap();
        let reserialized = serde_json::to_string(&result).unwrap();
        let decoded: TxProfileResult = serde_json::from_str(&reserialized).unwrap();
        assert_eq!(decoded.data["executionSteps"], 5);
    }

    #[test]
    fn block_hash_roundtrip() {
        let hash = BlockHash([0x11; 32]);
        let json = serde_json::to_string(&hash).unwrap();
        let decoded: BlockHash = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, hash);
    }

    #[test]
    fn log_id_roundtrip() {
        let log_id = sample_log_id();
        let json = serde_json::to_string(&log_id).unwrap();
        let decoded: LogId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, log_id);
    }

    #[test]
    fn utility_execution_result_roundtrip() {
        let result = UtilityExecutionResult {
            result: vec![Fr::from(1u64), Fr::from(2u64)],
            stats: Some(serde_json::json!({"timings": {"total": 1}})),
        };
        let json = serde_json::to_string(&result).unwrap();
        let decoded: UtilityExecutionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.result, result.result);
        assert_eq!(decoded.stats, result.stats);
    }

    #[test]
    fn simulate_tx_opts_defaults() {
        let opts = SimulateTxOpts::default();
        assert!(!opts.simulate_public);
        assert!(!opts.skip_tx_validation);
        assert!(!opts.skip_fee_enforcement);
        assert!(opts.overrides.is_none());
        assert!(opts.scopes.is_empty());

        let json = serde_json::to_value(&opts).unwrap();
        assert_eq!(json["simulatePublic"], false);
        assert_eq!(json["skipTxValidation"], false);
        assert_eq!(json["skipFeeEnforcement"], false);
    }

    #[test]
    fn profile_tx_opts_defaults() {
        let opts = ProfileTxOpts::default();
        assert_eq!(opts.profile_mode, ProfileMode::Full);
        assert!(opts.skip_proof_generation);
        assert!(opts.scopes.is_empty());
    }

    #[test]
    fn execute_utility_opts_defaults() {
        let opts = ExecuteUtilityOpts::default();
        assert!(opts.authwits.is_empty());
        assert!(opts.scopes.is_empty());
    }

    #[test]
    fn packed_private_event_roundtrip() {
        let event = PackedPrivateEvent {
            packed_event: vec![Fr::from(1u64), Fr::from(2u64)],
            tx_hash: TxHash::zero(),
            l2_block_number: 42,
            l2_block_hash: sample_block_hash(),
            event_selector: EventSelector(Fr::from(7u64)),
        };
        let json = serde_json::to_string(&event).unwrap();
        let decoded: PackedPrivateEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.packed_event.len(), 2);
        assert_eq!(decoded.l2_block_number, 42);
        assert_eq!(decoded.l2_block_hash, sample_block_hash());
    }

    #[test]
    fn packed_private_event_minimal() {
        let event = PackedPrivateEvent {
            packed_event: vec![],
            tx_hash: TxHash::zero(),
            l2_block_number: 0,
            l2_block_hash: BlockHash::default(),
            event_selector: EventSelector(Fr::zero()),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["txHash"], TxHash::zero().to_string());
        assert_eq!(json["l2BlockNumber"], 0);
    }

    #[test]
    fn private_event_filter_default_serializes_minimal() {
        let filter = PrivateEventFilter::default();
        let json = serde_json::to_value(&filter).unwrap();
        assert_eq!(
            json["contractAddress"],
            AztecAddress(Fr::zero()).to_string()
        );
        assert!(json.get("txHash").is_none());
        assert!(json.get("fromBlock").is_none());
        assert!(json.get("toBlock").is_none());
        assert!(json.get("afterLog").is_none());
        assert_eq!(json["scopes"], serde_json::json!([]));
    }

    #[test]
    fn private_event_filter_with_fields() {
        let filter = PrivateEventFilter {
            contract_address: AztecAddress(Fr::from(9u64)),
            tx_hash: Some(TxHash::zero()),
            from_block: Some(10),
            to_block: Some(20),
            after_log: Some(sample_log_id()),
            scopes: vec![AztecAddress(Fr::from(1u64))],
        };
        let json = serde_json::to_value(&filter).unwrap();
        assert_eq!(
            json["contractAddress"],
            AztecAddress(Fr::from(9u64)).to_string()
        );
        assert_eq!(json["txHash"], TxHash::zero().to_string());
        assert_eq!(json["fromBlock"], 10);
        assert_eq!(json["toBlock"], 20);
        assert!(json.get("afterLog").is_some());
        assert_eq!(json["scopes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn register_contract_request_roundtrip() {
        let request = RegisterContractRequest {
            instance: sample_instance(),
            artifact: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        let decoded: RegisterContractRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.instance.address, request.instance.address);
        assert!(decoded.artifact.is_none());
    }

    #[test]
    fn register_contract_request_with_artifact() {
        let request = RegisterContractRequest {
            instance: sample_instance(),
            artifact: Some(ContractArtifact {
                name: "TestContract".into(),
                functions: vec![],
                outputs: None,
                file_map: None,
            }),
        };
        let json = serde_json::to_string(&request).unwrap();
        let decoded: RegisterContractRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.artifact.unwrap().name, "TestContract");
    }

    // -- Trait object safety --

    #[test]
    fn pxe_is_object_safe() {
        fn _assert_object_safe(_: &dyn Pxe) {}
    }

    // -- Send + Sync --

    #[test]
    fn http_pxe_client_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<HttpPxeClient>();
    }

    // -- Factory --

    #[test]
    fn create_pxe_client_does_not_panic() {
        let _client = create_pxe_client("http://localhost:8080");
    }

    // -- Test helpers --

    fn sample_instance() -> ContractInstanceWithAddress {
        ContractInstanceWithAddress {
            address: AztecAddress(Fr::from(1u64)),
            inner: ContractInstance {
                version: 1,
                salt: Fr::from(42u64),
                deployer: AztecAddress(Fr::from(2u64)),
                current_contract_class_id: Fr::from(100u64),
                original_contract_class_id: Fr::from(100u64),
                initialization_hash: Fr::from(0u64),
                public_keys: PublicKeys::default(),
            },
        }
    }

    fn sample_block_header() -> BlockHeader {
        BlockHeader {
            data: serde_json::json!({"globalVariables": {"blockNumber": 1}}),
        }
    }

    fn sample_block_hash() -> BlockHash {
        BlockHash([0x22; 32])
    }

    fn sample_log_id() -> LogId {
        LogId {
            block_number: 42,
            block_hash: sample_block_hash(),
            tx_hash: TxHash::zero(),
            tx_index: 3,
            log_index: 7,
        }
    }

    // -- MockPxe --

    struct MockPxe {
        header_results: Mutex<Vec<Result<BlockHeader, Error>>>,
        accounts: Vec<CompleteAddress>,
        senders: Mutex<Vec<AztecAddress>>,
        contracts: Vec<AztecAddress>,
        call_count: AtomicUsize,
    }

    impl MockPxe {
        fn new_ready() -> Self {
            Self {
                header_results: Mutex::new(vec![Ok(sample_block_header())]),
                accounts: vec![],
                senders: Mutex::new(vec![]),
                contracts: vec![],
                call_count: AtomicUsize::new(0),
            }
        }

        fn with_accounts(mut self, accounts: Vec<CompleteAddress>) -> Self {
            self.accounts = accounts;
            self
        }

        fn with_contracts(mut self, contracts: Vec<AztecAddress>) -> Self {
            self.contracts = contracts;
            self
        }

        fn with_header_sequence(mut self, results: Vec<Result<BlockHeader, Error>>) -> Self {
            self.header_results = Mutex::new(results);
            self
        }
    }

    #[async_trait]
    impl Pxe for MockPxe {
        async fn get_synced_block_header(&self) -> Result<BlockHeader, Error> {
            let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
            let results = self.header_results.lock().unwrap();
            if idx < results.len() {
                match &results[idx] {
                    Ok(h) => Ok(h.clone()),
                    Err(e) => Err(Error::Transport(e.to_string())),
                }
            } else if let Some(last) = results.last() {
                match last {
                    Ok(h) => Ok(h.clone()),
                    Err(e) => Err(Error::Transport(e.to_string())),
                }
            } else {
                Err(Error::Transport("no mock results configured".into()))
            }
        }

        async fn get_contract_instance(
            &self,
            address: &AztecAddress,
        ) -> Result<Option<ContractInstanceWithAddress>, Error> {
            if *address == AztecAddress(Fr::from(1u64)) {
                Ok(Some(sample_instance()))
            } else {
                Ok(None)
            }
        }

        async fn get_contract_artifact(&self, _id: &Fr) -> Result<Option<ContractArtifact>, Error> {
            Ok(None)
        }

        async fn get_contracts(&self) -> Result<Vec<AztecAddress>, Error> {
            Ok(self.contracts.clone())
        }

        async fn register_account(
            &self,
            _secret_key: &Fr,
            _partial_address: &Fr,
        ) -> Result<CompleteAddress, Error> {
            Ok(CompleteAddress::default())
        }

        async fn get_registered_accounts(&self) -> Result<Vec<CompleteAddress>, Error> {
            Ok(self.accounts.clone())
        }

        async fn register_sender(&self, sender: &AztecAddress) -> Result<AztecAddress, Error> {
            self.senders.lock().unwrap().push(*sender);
            Ok(*sender)
        }

        async fn get_senders(&self) -> Result<Vec<AztecAddress>, Error> {
            Ok(self.senders.lock().unwrap().clone())
        }

        async fn remove_sender(&self, sender: &AztecAddress) -> Result<(), Error> {
            self.senders.lock().unwrap().retain(|s| s != sender);
            Ok(())
        }

        async fn register_contract_class(&self, _artifact: &ContractArtifact) -> Result<(), Error> {
            Ok(())
        }

        async fn register_contract(&self, _request: RegisterContractRequest) -> Result<(), Error> {
            Ok(())
        }

        async fn update_contract(
            &self,
            _address: &AztecAddress,
            _artifact: &ContractArtifact,
        ) -> Result<(), Error> {
            Ok(())
        }

        async fn simulate_tx(
            &self,
            _tx_request: &TxExecutionRequest,
            _opts: SimulateTxOpts,
        ) -> Result<TxSimulationResult, Error> {
            Ok(TxSimulationResult {
                data: serde_json::json!({"returnValues": []}),
            })
        }

        async fn prove_tx(
            &self,
            _tx_request: &TxExecutionRequest,
            _scopes: Vec<AztecAddress>,
        ) -> Result<TxProvingResult, Error> {
            Ok(TxProvingResult {
                data: serde_json::json!({"proof": "0x00"}),
            })
        }

        async fn profile_tx(
            &self,
            _tx_request: &TxExecutionRequest,
            _opts: ProfileTxOpts,
        ) -> Result<TxProfileResult, Error> {
            Ok(TxProfileResult {
                data: serde_json::json!({"gateCounts": []}),
            })
        }

        async fn execute_utility(
            &self,
            _call: &FunctionCall,
            _opts: ExecuteUtilityOpts,
        ) -> Result<UtilityExecutionResult, Error> {
            Ok(UtilityExecutionResult {
                result: vec![],
                stats: None,
            })
        }

        async fn get_private_events(
            &self,
            _event_selector: &EventSelector,
            _filter: PrivateEventFilter,
        ) -> Result<Vec<PackedPrivateEvent>, Error> {
            Ok(vec![])
        }

        async fn stop(&self) -> Result<(), Error> {
            Ok(())
        }
    }

    // -- Mock-based async tests --

    #[tokio::test]
    async fn mock_get_synced_block_header() {
        let pxe = MockPxe::new_ready();
        let header = pxe.get_synced_block_header().await.unwrap();
        assert_eq!(header.data["globalVariables"]["blockNumber"], 1);
    }

    #[tokio::test]
    async fn mock_register_account() {
        let pxe = MockPxe::new_ready();
        let result = pxe
            .register_account(&Fr::from(1u64), &Fr::from(2u64))
            .await
            .unwrap();
        assert_eq!(result, CompleteAddress::default());
    }

    #[tokio::test]
    async fn mock_get_registered_accounts() {
        let account = CompleteAddress {
            address: AztecAddress(Fr::from(99u64)),
            ..CompleteAddress::default()
        };
        let pxe = MockPxe::new_ready().with_accounts(vec![account.clone()]);
        let accounts = pxe.get_registered_accounts().await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].address, AztecAddress(Fr::from(99u64)));
    }

    #[tokio::test]
    async fn mock_register_and_get_senders() {
        let pxe = MockPxe::new_ready();
        let addr = AztecAddress(Fr::from(42u64));

        let result = pxe.register_sender(&addr).await.unwrap();
        assert_eq!(result, addr);

        let senders = pxe.get_senders().await.unwrap();
        assert_eq!(senders.len(), 1);
        assert_eq!(senders[0], addr);
    }

    #[tokio::test]
    async fn mock_remove_sender() {
        let pxe = MockPxe::new_ready();
        let addr = AztecAddress(Fr::from(42u64));

        pxe.register_sender(&addr).await.unwrap();
        assert_eq!(pxe.get_senders().await.unwrap().len(), 1);

        pxe.remove_sender(&addr).await.unwrap();
        assert!(pxe.get_senders().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn mock_get_contract_instance_found() {
        let pxe = MockPxe::new_ready();
        let result = pxe
            .get_contract_instance(&AztecAddress(Fr::from(1u64)))
            .await
            .unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().address, AztecAddress(Fr::from(1u64)));
    }

    #[tokio::test]
    async fn mock_get_contract_instance_not_found() {
        let pxe = MockPxe::new_ready();
        let result = pxe
            .get_contract_instance(&AztecAddress(Fr::from(999u64)))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn mock_get_contracts() {
        let pxe = MockPxe::new_ready().with_contracts(vec![
            AztecAddress(Fr::from(1u64)),
            AztecAddress(Fr::from(2u64)),
        ]);
        let contracts = pxe.get_contracts().await.unwrap();
        assert_eq!(contracts.len(), 2);
    }

    #[tokio::test]
    async fn mock_simulate_tx() {
        let pxe = MockPxe::new_ready();
        let req = TxExecutionRequest {
            data: serde_json::json!({}),
        };
        let result = pxe
            .simulate_tx(&req, SimulateTxOpts::default())
            .await
            .unwrap();
        assert_eq!(result.data["returnValues"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn mock_execute_utility() {
        let pxe = MockPxe::new_ready();
        let call = FunctionCall {
            to: AztecAddress(Fr::from(1u64)),
            selector: FunctionSelector::from_hex("0xaabbccdd").unwrap(),
            args: vec![],
            function_type: FunctionType::Utility,
            is_static: true,
        };
        let result = pxe
            .execute_utility(&call, ExecuteUtilityOpts::default())
            .await
            .unwrap();
        assert!(result.result.is_empty());
        assert!(result.stats.is_none());
    }

    #[tokio::test]
    async fn mock_get_private_events_empty() {
        let pxe = MockPxe::new_ready();
        let events = pxe
            .get_private_events(
                &EventSelector(Fr::from(1u64)),
                PrivateEventFilter {
                    contract_address: AztecAddress(Fr::from(1u64)),
                    scopes: vec![AztecAddress(Fr::from(2u64))],
                    ..PrivateEventFilter::default()
                },
            )
            .await
            .unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn mock_register_contract() {
        let pxe = MockPxe::new_ready();
        let request = RegisterContractRequest {
            instance: sample_instance(),
            artifact: None,
        };
        pxe.register_contract(request).await.unwrap();
    }

    #[tokio::test]
    async fn mock_stop() {
        let pxe = MockPxe::new_ready();
        pxe.stop().await.unwrap();
    }

    // -- wait_for_pxe tests --

    #[tokio::test]
    async fn wait_for_pxe_immediate_success() {
        let pxe = MockPxe::new_ready();
        let header = wait_for_pxe_opts(&pxe, Duration::from_secs(5), Duration::from_millis(10))
            .await
            .unwrap();
        assert_eq!(header.data["globalVariables"]["blockNumber"], 1);
    }

    #[tokio::test]
    async fn wait_for_pxe_delayed_success() {
        let pxe = MockPxe::new_ready().with_header_sequence(vec![
            Err(Error::Transport("not ready".into())),
            Err(Error::Transport("not ready".into())),
            Ok(sample_block_header()),
        ]);
        let header = wait_for_pxe_opts(&pxe, Duration::from_secs(5), Duration::from_millis(10))
            .await
            .unwrap();
        assert_eq!(header.data["globalVariables"]["blockNumber"], 1);
    }

    #[tokio::test]
    async fn wait_for_pxe_timeout() {
        let pxe = MockPxe::new_ready()
            .with_header_sequence(vec![Err(Error::Transport("not ready".into()))]);
        let result =
            wait_for_pxe_opts(&pxe, Duration::from_millis(50), Duration::from_millis(100)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::Timeout(_)));
    }
}
