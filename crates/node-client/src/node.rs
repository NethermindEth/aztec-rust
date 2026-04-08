use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::abi::EventSelector;
use crate::error::Error;
use crate::rpc::RpcTransport;
use crate::tx::{TxHash, TxReceipt, TxStatus};
use crate::types::{AztecAddress, ContractInstanceWithAddress, Fr};

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Information returned by the Aztec node about its current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    /// Semantic version of the node software.
    pub node_version: String,
    /// L1 chain ID the node is connected to.
    pub l1_chain_id: u64,
    /// Rollup protocol version.
    pub rollup_version: u64,
    /// Ethereum Node Record, if available.
    #[serde(default)]
    pub enr: Option<String>,
    /// L1 contract addresses — kept as opaque JSON until the full schema stabilizes.
    #[serde(default)]
    pub l1_contract_addresses: serde_json::Value,
    /// Protocol contract addresses — kept as opaque JSON until the full schema stabilizes.
    #[serde(default)]
    pub protocol_contract_addresses: serde_json::Value,
    /// Whether the node is running with real (non-mock) proofs.
    pub real_proofs: bool,
}

/// Identifies a specific log entry within a block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogId {
    /// Block number containing the log.
    pub block_number: u64,
    /// Index of the log within the block.
    pub log_index: u64,
}

/// Filter for querying public logs from the node.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicLogFilter {
    /// Filter by transaction hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<TxHash>,
    /// Start block (inclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_block: Option<u64>,
    /// End block (inclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_block: Option<u64>,
    /// Filter by emitting contract address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_address: Option<AztecAddress>,
    /// Filter by event selector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<EventSelector>,
    /// Cursor for pagination — fetch logs after this entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_log: Option<LogId>,
}

/// A single public log entry returned by the node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicLog {
    /// Address of the contract that emitted the log.
    pub contract_address: AztecAddress,
    /// Field element data comprising the log payload.
    pub data: Vec<Fr>,
    /// Hash of the transaction that emitted the log.
    #[serde(default)]
    pub tx_hash: Option<TxHash>,
    /// Block number containing the log.
    pub block_number: u64,
    /// Index of the log within the block.
    pub log_index: u64,
}

/// Response from a public logs query.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicLogsResponse {
    /// Matching log entries.
    pub logs: Vec<PublicLog>,
    /// Whether the response was truncated due to the log limit.
    pub max_logs_hit: bool,
}

/// Options controlling `wait_for_tx` polling behavior.
#[derive(Debug, Clone)]
pub struct WaitOpts {
    /// Total timeout for the polling operation.
    pub timeout: Duration,
    /// Interval between retries.
    pub interval: Duration,
    /// Minimum status to wait for.
    pub wait_for_status: TxStatus,
    /// If true, accept a reverted tx receipt without returning an error.
    pub dont_throw_on_revert: bool,
    /// Duration to ignore `TxStatus::Dropped` before treating it as failure.
    /// Avoids race conditions where a tx briefly appears dropped before inclusion.
    pub ignore_dropped_receipts_for: Duration,
}

impl Default for WaitOpts {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(300),
            interval: Duration::from_secs(1),
            wait_for_status: TxStatus::Checkpointed,
            dont_throw_on_revert: false,
            ignore_dropped_receipts_for: Duration::from_secs(5),
        }
    }
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Public read interface for an Aztec node.
#[async_trait]
pub trait AztecNode: Send + Sync {
    /// Fetch information about the node's current state.
    async fn get_node_info(&self) -> Result<NodeInfo, Error>;
    /// Get the latest block number.
    async fn get_block_number(&self) -> Result<u64, Error>;
    /// Get the latest proven block number.
    async fn get_proven_block_number(&self) -> Result<u64, Error>;
    /// Get the receipt for a transaction.
    async fn get_tx_receipt(&self, tx_hash: &TxHash) -> Result<TxReceipt, Error>;
    /// Query public logs matching the given filter.
    async fn get_public_logs(&self, filter: PublicLogFilter) -> Result<PublicLogsResponse, Error>;
    /// Send a proven transaction to the network for inclusion.
    async fn send_tx(&self, tx: &serde_json::Value) -> Result<TxHash, Error>;
    /// Get a publicly deployed contract instance by address.
    async fn get_contract(
        &self,
        address: &AztecAddress,
    ) -> Result<Option<ContractInstanceWithAddress>, Error>;
    /// Get a publicly registered contract class by ID.
    async fn get_contract_class(&self, id: &Fr) -> Result<Option<serde_json::Value>, Error>;
}

// ---------------------------------------------------------------------------
// HTTP client
// ---------------------------------------------------------------------------

/// HTTP JSON-RPC backed Aztec node client.
pub struct HttpNodeClient {
    transport: RpcTransport,
}

impl HttpNodeClient {
    fn new(url: String, timeout: Duration) -> Self {
        Self {
            transport: RpcTransport::new(url, timeout),
        }
    }
}

#[async_trait]
impl AztecNode for HttpNodeClient {
    async fn get_node_info(&self) -> Result<NodeInfo, Error> {
        self.transport
            .call("node_getNodeInfo", serde_json::json!([]))
            .await
    }

    async fn get_block_number(&self) -> Result<u64, Error> {
        self.transport
            .call("node_getBlockNumber", serde_json::json!([]))
            .await
    }

    async fn get_proven_block_number(&self) -> Result<u64, Error> {
        self.transport
            .call("node_getProvenBlockNumber", serde_json::json!([]))
            .await
    }

    async fn get_tx_receipt(&self, tx_hash: &TxHash) -> Result<TxReceipt, Error> {
        self.transport
            .call("node_getTxReceipt", serde_json::json!([tx_hash]))
            .await
    }

    async fn get_public_logs(&self, filter: PublicLogFilter) -> Result<PublicLogsResponse, Error> {
        self.transport
            .call("node_getPublicLogs", serde_json::json!([filter]))
            .await
    }

    async fn send_tx(&self, tx: &serde_json::Value) -> Result<TxHash, Error> {
        self.transport
            .call("node_sendTx", serde_json::json!([tx]))
            .await
    }

    async fn get_contract(
        &self,
        address: &AztecAddress,
    ) -> Result<Option<ContractInstanceWithAddress>, Error> {
        self.transport
            .call("node_getContract", serde_json::json!([address]))
            .await
    }

    async fn get_contract_class(&self, id: &Fr) -> Result<Option<serde_json::Value>, Error> {
        self.transport
            .call("node_getContractClass", serde_json::json!([id]))
            .await
    }
}

/// Create an HTTP JSON-RPC backed Aztec node client.
pub fn create_aztec_node_client(url: impl Into<String>) -> HttpNodeClient {
    HttpNodeClient::new(url.into(), Duration::from_secs(30))
}

// ---------------------------------------------------------------------------
// Polling helpers
// ---------------------------------------------------------------------------

/// Wait for the node to become ready by retrying `get_node_info`.
///
/// Uses a default timeout of 120 seconds with a 1 second polling interval.
/// Returns the `NodeInfo` on success, or a timeout error.
pub async fn wait_for_node(node: &(impl AztecNode + ?Sized)) -> Result<NodeInfo, Error> {
    wait_for_node_opts(node, Duration::from_secs(120), Duration::from_secs(1)).await
}

async fn wait_for_node_opts(
    node: &(impl AztecNode + ?Sized),
    timeout: Duration,
    interval: Duration,
) -> Result<NodeInfo, Error> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match node.get_node_info().await {
            Ok(info) => return Ok(info),
            Err(_) if tokio::time::Instant::now() + interval < deadline => {
                tokio::time::sleep(interval).await;
            }
            Err(e) => {
                return Err(Error::Timeout(format!(
                    "node not ready after {timeout:?}: {e}"
                )));
            }
        }
    }
}

/// Returns `true` if `status` meets or exceeds `target` in the tx lifecycle progression.
const fn status_reached(status: TxStatus, target: TxStatus) -> bool {
    status_ordinal(status) >= status_ordinal(target)
}

const fn status_ordinal(s: TxStatus) -> u8 {
    match s {
        TxStatus::Dropped => 0,
        TxStatus::Pending => 1,
        TxStatus::Proposed => 2,
        TxStatus::Checkpointed => 3,
        TxStatus::Proven => 4,
        TxStatus::Finalized => 5,
    }
}

/// Wait for a transaction to reach a terminal status by polling `get_tx_receipt`.
///
/// By default, waits until `Checkpointed` or higher. Set `opts.wait_for_status`
/// to target a different status. Returns early with an error on `Dropped`
/// (after the grace period) or reverted execution results (unless
/// `opts.dont_throw_on_revert` is set).
pub async fn wait_for_tx(
    node: &(impl AztecNode + ?Sized),
    tx_hash: &TxHash,
    opts: WaitOpts,
) -> Result<TxReceipt, Error> {
    let start = tokio::time::Instant::now();
    let deadline = start + opts.timeout;
    let target = opts.wait_for_status;

    loop {
        match node.get_tx_receipt(tx_hash).await {
            Ok(receipt) => {
                if receipt.is_dropped() {
                    let elapsed = start.elapsed();
                    if elapsed >= opts.ignore_dropped_receipts_for {
                        return Err(Error::Reverted(format!(
                            "tx {tx_hash} was dropped: {}",
                            receipt.error.as_deref().unwrap_or("unknown reason")
                        )));
                    }
                    // Within grace period — keep polling.
                } else {
                    if receipt.has_execution_reverted() && !opts.dont_throw_on_revert {
                        return Err(Error::Reverted(format!(
                            "tx {tx_hash} execution reverted: {}",
                            receipt.error.as_deref().unwrap_or("unknown reason")
                        )));
                    }
                    if status_reached(receipt.status, target) {
                        return Ok(receipt);
                    }
                }
            }
            Err(e) => {
                if tokio::time::Instant::now() + opts.interval >= deadline {
                    return Err(Error::Timeout(format!(
                        "timed out waiting for tx {tx_hash}: {e}"
                    )));
                }
            }
        }

        if tokio::time::Instant::now() + opts.interval >= deadline {
            return Err(Error::Timeout(format!(
                "tx {tx_hash} did not reach {target:?} within {:?}",
                opts.timeout
            )));
        }
        tokio::time::sleep(opts.interval).await;
    }
}

/// Options for [`wait_for_proven`].
#[derive(Debug, Clone)]
pub struct WaitForProvenOpts {
    /// Timeout for proven status polling (default: 600s / 10 minutes).
    pub proven_timeout: Duration,
    /// Polling interval (default: 1s).
    pub interval: Duration,
}

impl Default for WaitForProvenOpts {
    fn default() -> Self {
        Self {
            proven_timeout: Duration::from_secs(600),
            interval: Duration::from_secs(1),
        }
    }
}

/// Wait until the block containing the given receipt is proven on L1.
///
/// Takes a receipt (which must have a `block_number`) and polls the node
/// until the proven block number >= receipt's block number.
/// Returns the proven block number on success.
pub async fn wait_for_proven(
    node: &(impl AztecNode + ?Sized),
    receipt: &TxReceipt,
    opts: WaitForProvenOpts,
) -> Result<u64, Error> {
    let receipt_block = receipt.block_number.ok_or_else(|| {
        Error::InvalidData("receipt has no block_number — cannot wait for proven".into())
    })?;

    let deadline = tokio::time::Instant::now() + opts.proven_timeout;

    loop {
        match node.get_proven_block_number().await {
            Ok(proven_block) if proven_block >= receipt_block => {
                return Ok(proven_block);
            }
            Ok(_) => {}
            Err(e) => {
                if tokio::time::Instant::now() + opts.interval >= deadline {
                    return Err(Error::Timeout(format!(
                        "timed out waiting for block {receipt_block} to be proven: {e}"
                    )));
                }
            }
        }

        if tokio::time::Instant::now() + opts.interval >= deadline {
            return Err(Error::Timeout(format!(
                "block {receipt_block} was not proven within {:?}",
                opts.proven_timeout
            )));
        }
        tokio::time::sleep(opts.interval).await;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::tx::TxExecutionResult;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    // -- NodeInfo fixture deserialization --

    #[test]
    fn node_info_deserializes() {
        let json = r#"{
            "nodeVersion": "0.42.0",
            "l1ChainId": 31337,
            "rollupVersion": 1,
            "enr": "enr:-abc123",
            "l1ContractAddresses": {"rollup": "0x1234"},
            "protocolContractAddresses": {"classRegisterer": "0xabcd"},
            "realProofs": false
        }"#;

        let info: NodeInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.node_version, "0.42.0");
        assert_eq!(info.l1_chain_id, 31337);
        assert_eq!(info.rollup_version, 1);
        assert_eq!(info.enr.as_deref(), Some("enr:-abc123"));
        assert!(!info.real_proofs);
    }

    #[test]
    fn node_info_roundtrip() {
        let info = NodeInfo {
            node_version: "1.0.0".into(),
            l1_chain_id: 1,
            rollup_version: 2,
            enr: None,
            l1_contract_addresses: serde_json::json!({}),
            protocol_contract_addresses: serde_json::json!({}),
            real_proofs: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        let decoded: NodeInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.node_version, "1.0.0");
        assert_eq!(decoded.l1_chain_id, 1);
        assert_eq!(decoded.rollup_version, 2);
        assert!(decoded.real_proofs);
    }

    #[test]
    fn node_info_minimal_json() {
        let json = r#"{
            "nodeVersion": "0.1.0",
            "l1ChainId": 1,
            "rollupVersion": 1,
            "realProofs": false
        }"#;
        let info: NodeInfo = serde_json::from_str(json).unwrap();
        assert!(info.enr.is_none());
        assert_eq!(info.rollup_version, 1);
    }

    // -- PublicLogFilter --

    #[test]
    fn public_log_filter_default_serializes_empty() {
        let filter = PublicLogFilter::default();
        let json = serde_json::to_value(&filter).unwrap();
        assert_eq!(json, serde_json::json!({}));
    }

    #[test]
    fn public_log_filter_with_fields() {
        let tx_hash =
            TxHash::from_hex("0x0000000000000000000000000000000000000000000000000000000000000001")
                .unwrap();
        let filter = PublicLogFilter {
            tx_hash: Some(tx_hash),
            from_block: Some(10),
            to_block: Some(20),
            ..Default::default()
        };
        let json = serde_json::to_value(&filter).unwrap();
        assert_eq!(json["txHash"], tx_hash.to_string());
        assert_eq!(json["fromBlock"], 10);
        assert_eq!(json["toBlock"], 20);
        assert!(json.get("contractAddress").is_none());
    }

    // -- PublicLogsResponse --

    #[test]
    fn public_logs_response_roundtrip() {
        let resp = PublicLogsResponse {
            logs: vec![],
            max_logs_hit: false,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let decoded: PublicLogsResponse = serde_json::from_str(&json).unwrap();
        assert!(!decoded.max_logs_hit);
        assert!(decoded.logs.is_empty());
    }

    // -- WaitOpts --

    #[test]
    fn wait_opts_defaults() {
        let opts = WaitOpts::default();
        assert_eq!(opts.timeout, Duration::from_secs(300));
        assert_eq!(opts.interval, Duration::from_secs(1));
        assert_eq!(opts.wait_for_status, TxStatus::Checkpointed);
        assert!(!opts.dont_throw_on_revert);
        assert_eq!(opts.ignore_dropped_receipts_for, Duration::from_secs(5));
    }

    // -- Status ordering --

    #[test]
    fn status_ordering() {
        assert!(status_reached(TxStatus::Finalized, TxStatus::Checkpointed));
        assert!(status_reached(TxStatus::Proven, TxStatus::Checkpointed));
        assert!(status_reached(
            TxStatus::Checkpointed,
            TxStatus::Checkpointed
        ));
        assert!(!status_reached(TxStatus::Proposed, TxStatus::Checkpointed));
        assert!(!status_reached(TxStatus::Pending, TxStatus::Checkpointed));
        assert!(!status_reached(TxStatus::Dropped, TxStatus::Checkpointed));

        assert!(status_reached(TxStatus::Finalized, TxStatus::Proven));
        assert!(status_reached(TxStatus::Proven, TxStatus::Proven));
        assert!(!status_reached(TxStatus::Checkpointed, TxStatus::Proven));
    }

    // -- Mock node for trait-based tests --

    struct MockNode {
        info_result: Mutex<Vec<Result<NodeInfo, Error>>>,
        block_number: u64,
        proven_block_results: Mutex<Vec<Result<u64, Error>>>,
        receipt_results: Mutex<Vec<Result<TxReceipt, Error>>>,
        call_count: AtomicUsize,
    }

    impl MockNode {
        fn new_ready(info: NodeInfo) -> Self {
            Self {
                info_result: Mutex::new(vec![Ok(info)]),
                block_number: 0,
                proven_block_results: Mutex::new(vec![]),
                receipt_results: Mutex::new(vec![]),
                call_count: AtomicUsize::new(0),
            }
        }

        fn new_with_info_sequence(results: Vec<Result<NodeInfo, Error>>) -> Self {
            Self {
                info_result: Mutex::new(results),
                block_number: 0,
                proven_block_results: Mutex::new(vec![]),
                receipt_results: Mutex::new(vec![]),
                call_count: AtomicUsize::new(0),
            }
        }

        fn new_with_receipt_sequence(results: Vec<Result<TxReceipt, Error>>) -> Self {
            Self {
                info_result: Mutex::new(vec![]),
                block_number: 0,
                proven_block_results: Mutex::new(vec![]),
                receipt_results: Mutex::new(results),
                call_count: AtomicUsize::new(0),
            }
        }

        fn new_with_proven_block_sequence(results: Vec<Result<u64, Error>>) -> Self {
            Self {
                info_result: Mutex::new(vec![]),
                block_number: 0,
                proven_block_results: Mutex::new(results),
                receipt_results: Mutex::new(vec![]),
                call_count: AtomicUsize::new(0),
            }
        }

        fn sample_info() -> NodeInfo {
            NodeInfo {
                node_version: "test-0.1.0".into(),
                l1_chain_id: 31337,
                rollup_version: 1,
                enr: None,
                l1_contract_addresses: serde_json::json!({}),
                protocol_contract_addresses: serde_json::json!({}),
                real_proofs: false,
            }
        }

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
    }

    #[async_trait]
    impl AztecNode for MockNode {
        async fn get_node_info(&self) -> Result<NodeInfo, Error> {
            let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
            let results = self.info_result.lock().unwrap();
            if idx < results.len() {
                match &results[idx] {
                    Ok(info) => Ok(info.clone()),
                    Err(e) => Err(Error::Transport(e.to_string())),
                }
            } else if let Some(last) = results.last() {
                match last {
                    Ok(info) => Ok(info.clone()),
                    Err(e) => Err(Error::Transport(e.to_string())),
                }
            } else {
                Err(Error::Transport("no mock results configured".into()))
            }
        }

        async fn get_block_number(&self) -> Result<u64, Error> {
            Ok(self.block_number)
        }

        async fn get_proven_block_number(&self) -> Result<u64, Error> {
            let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
            let results = self.proven_block_results.lock().unwrap();
            if idx < results.len() {
                match &results[idx] {
                    Ok(n) => Ok(*n),
                    Err(e) => Err(Error::Transport(e.to_string())),
                }
            } else if let Some(last) = results.last() {
                match last {
                    Ok(n) => Ok(*n),
                    Err(e) => Err(Error::Transport(e.to_string())),
                }
            } else {
                Ok(0)
            }
        }

        async fn get_tx_receipt(&self, _tx_hash: &TxHash) -> Result<TxReceipt, Error> {
            let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
            let results = self.receipt_results.lock().unwrap();
            if idx < results.len() {
                match &results[idx] {
                    Ok(r) => Ok(r.clone()),
                    Err(e) => Err(Error::Transport(e.to_string())),
                }
            } else if let Some(last) = results.last() {
                match last {
                    Ok(r) => Ok(r.clone()),
                    Err(e) => Err(Error::Transport(e.to_string())),
                }
            } else {
                Err(Error::Transport("no mock results configured".into()))
            }
        }

        async fn get_public_logs(
            &self,
            _filter: PublicLogFilter,
        ) -> Result<PublicLogsResponse, Error> {
            Ok(PublicLogsResponse {
                logs: vec![],
                max_logs_hit: false,
            })
        }

        async fn send_tx(&self, _tx: &serde_json::Value) -> Result<TxHash, Error> {
            Ok(TxHash::zero())
        }

        async fn get_contract(
            &self,
            _address: &AztecAddress,
        ) -> Result<Option<ContractInstanceWithAddress>, Error> {
            Ok(None)
        }

        async fn get_contract_class(&self, _id: &Fr) -> Result<Option<serde_json::Value>, Error> {
            Ok(None)
        }
    }

    // -- Mock-based RPC tests --

    #[tokio::test]
    async fn mock_get_node_info() {
        let node = MockNode::new_ready(MockNode::sample_info());
        let info = node.get_node_info().await.unwrap();
        assert_eq!(info.node_version, "test-0.1.0");
        assert_eq!(info.l1_chain_id, 31337);
    }

    #[tokio::test]
    async fn mock_get_block_number() {
        let node = MockNode {
            block_number: 42,
            ..MockNode::new_ready(MockNode::sample_info())
        };
        let bn = node.get_block_number().await.unwrap();
        assert_eq!(bn, 42);
    }

    #[tokio::test]
    async fn mock_get_tx_receipt() {
        let receipt =
            MockNode::make_receipt(TxStatus::Checkpointed, Some(TxExecutionResult::Success));
        let node = MockNode::new_with_receipt_sequence(vec![Ok(receipt.clone())]);
        let result = node.get_tx_receipt(&TxHash::zero()).await.unwrap();
        assert_eq!(result.status, TxStatus::Checkpointed);
        assert!(result.has_execution_succeeded());
    }

    #[tokio::test]
    async fn mock_get_public_logs() {
        let node = MockNode::new_ready(MockNode::sample_info());
        let resp = node
            .get_public_logs(PublicLogFilter::default())
            .await
            .unwrap();
        assert!(resp.logs.is_empty());
        assert!(!resp.max_logs_hit);
    }

    // -- wait_for_node tests --

    #[tokio::test]
    async fn wait_for_node_immediate_success() {
        let node = MockNode::new_ready(MockNode::sample_info());
        let info = wait_for_node_opts(&node, Duration::from_secs(5), Duration::from_millis(10))
            .await
            .unwrap();
        assert_eq!(info.node_version, "test-0.1.0");
    }

    #[tokio::test]
    async fn wait_for_node_delayed_success() {
        let node = MockNode::new_with_info_sequence(vec![
            Err(Error::Transport("not ready".into())),
            Err(Error::Transport("not ready".into())),
            Ok(MockNode::sample_info()),
        ]);
        let info = wait_for_node_opts(&node, Duration::from_secs(5), Duration::from_millis(10))
            .await
            .unwrap();
        assert_eq!(info.node_version, "test-0.1.0");
    }

    #[tokio::test]
    async fn wait_for_node_timeout() {
        let node =
            MockNode::new_with_info_sequence(vec![Err(Error::Transport("not ready".into()))]);
        let result =
            wait_for_node_opts(&node, Duration::from_millis(50), Duration::from_millis(100)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, Error::Timeout(_)));
    }

    // -- wait_for_tx tests --

    #[tokio::test]
    async fn wait_for_tx_immediate_success() {
        let receipt =
            MockNode::make_receipt(TxStatus::Checkpointed, Some(TxExecutionResult::Success));
        let node = MockNode::new_with_receipt_sequence(vec![Ok(receipt)]);
        let opts = WaitOpts {
            timeout: Duration::from_secs(5),
            interval: Duration::from_millis(10),
            ..WaitOpts::default()
        };
        let result = wait_for_tx(&node, &TxHash::zero(), opts).await.unwrap();
        assert_eq!(result.status, TxStatus::Checkpointed);
    }

    #[tokio::test]
    async fn wait_for_tx_delayed_success() {
        let pending = MockNode::make_receipt(TxStatus::Pending, None);
        let proposed = MockNode::make_receipt(TxStatus::Proposed, Some(TxExecutionResult::Success));
        let checkpointed =
            MockNode::make_receipt(TxStatus::Checkpointed, Some(TxExecutionResult::Success));

        let node =
            MockNode::new_with_receipt_sequence(vec![Ok(pending), Ok(proposed), Ok(checkpointed)]);
        let opts = WaitOpts {
            timeout: Duration::from_secs(5),
            interval: Duration::from_millis(10),
            ..WaitOpts::default()
        };
        let result = wait_for_tx(&node, &TxHash::zero(), opts).await.unwrap();
        assert_eq!(result.status, TxStatus::Checkpointed);
    }

    #[tokio::test]
    async fn wait_for_tx_proven() {
        let checkpointed =
            MockNode::make_receipt(TxStatus::Checkpointed, Some(TxExecutionResult::Success));
        let proven = MockNode::make_receipt(TxStatus::Proven, Some(TxExecutionResult::Success));

        let node = MockNode::new_with_receipt_sequence(vec![Ok(checkpointed), Ok(proven)]);
        let opts = WaitOpts {
            timeout: Duration::from_secs(5),
            interval: Duration::from_millis(10),
            wait_for_status: TxStatus::Proven,
            ..WaitOpts::default()
        };
        let result = wait_for_tx(&node, &TxHash::zero(), opts).await.unwrap();
        assert_eq!(result.status, TxStatus::Proven);
    }

    #[tokio::test]
    async fn wait_for_tx_dropped() {
        let receipt = MockNode::make_receipt(TxStatus::Dropped, None);
        let node = MockNode::new_with_receipt_sequence(vec![Ok(receipt)]);
        let opts = WaitOpts {
            timeout: Duration::from_secs(5),
            interval: Duration::from_millis(10),
            ignore_dropped_receipts_for: Duration::ZERO,
            ..WaitOpts::default()
        };
        let result = wait_for_tx(&node, &TxHash::zero(), opts).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Reverted(_)));
    }

    #[tokio::test]
    async fn wait_for_tx_reverted() {
        let receipt = MockNode::make_receipt(
            TxStatus::Checkpointed,
            Some(TxExecutionResult::AppLogicReverted),
        );
        let node = MockNode::new_with_receipt_sequence(vec![Ok(receipt)]);
        let opts = WaitOpts {
            timeout: Duration::from_secs(5),
            interval: Duration::from_millis(10),
            ..WaitOpts::default()
        };
        let result = wait_for_tx(&node, &TxHash::zero(), opts).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Reverted(_)));
    }

    #[tokio::test]
    async fn wait_for_tx_timeout() {
        let pending = MockNode::make_receipt(TxStatus::Pending, None);
        let node = MockNode::new_with_receipt_sequence(vec![Ok(pending)]);
        let opts = WaitOpts {
            timeout: Duration::from_millis(50),
            interval: Duration::from_millis(100),
            ..WaitOpts::default()
        };
        let result = wait_for_tx(&node, &TxHash::zero(), opts).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Timeout(_)));
    }

    // -- Receipt progression tests --

    #[tokio::test]
    async fn wait_for_tx_finalized_exceeds_checkpointed() {
        let receipt = MockNode::make_receipt(TxStatus::Finalized, Some(TxExecutionResult::Success));
        let node = MockNode::new_with_receipt_sequence(vec![Ok(receipt)]);
        let opts = WaitOpts {
            timeout: Duration::from_secs(5),
            interval: Duration::from_millis(10),
            ..WaitOpts::default()
        };
        let result = wait_for_tx(&node, &TxHash::zero(), opts).await.unwrap();
        assert_eq!(result.status, TxStatus::Finalized);
    }

    #[tokio::test]
    async fn wait_for_tx_respects_wait_for_status() {
        let proposed = MockNode::make_receipt(TxStatus::Proposed, Some(TxExecutionResult::Success));
        let proven = MockNode::make_receipt(TxStatus::Proven, Some(TxExecutionResult::Success));
        let node = MockNode::new_with_receipt_sequence(vec![Ok(proposed), Ok(proven)]);
        let opts = WaitOpts {
            timeout: Duration::from_secs(5),
            interval: Duration::from_millis(10),
            wait_for_status: TxStatus::Proven,
            ..WaitOpts::default()
        };
        let result = wait_for_tx(&node, &TxHash::zero(), opts).await.unwrap();
        assert_eq!(result.status, TxStatus::Proven);
    }

    #[tokio::test]
    async fn wait_for_tx_dont_throw_on_revert() {
        let receipt = MockNode::make_receipt(
            TxStatus::Checkpointed,
            Some(TxExecutionResult::AppLogicReverted),
        );
        let node = MockNode::new_with_receipt_sequence(vec![Ok(receipt)]);
        let opts = WaitOpts {
            timeout: Duration::from_secs(5),
            interval: Duration::from_millis(10),
            dont_throw_on_revert: true,
            ..WaitOpts::default()
        };
        let result = wait_for_tx(&node, &TxHash::zero(), opts).await.unwrap();
        assert_eq!(result.status, TxStatus::Checkpointed);
        assert!(result.has_execution_reverted());
    }

    #[tokio::test]
    async fn wait_for_tx_ignores_dropped_within_grace_period() {
        let dropped = MockNode::make_receipt(TxStatus::Dropped, None);
        let checkpointed =
            MockNode::make_receipt(TxStatus::Checkpointed, Some(TxExecutionResult::Success));
        let node = MockNode::new_with_receipt_sequence(vec![Ok(dropped), Ok(checkpointed)]);
        let opts = WaitOpts {
            timeout: Duration::from_secs(5),
            interval: Duration::from_millis(10),
            ignore_dropped_receipts_for: Duration::from_secs(60),
            ..WaitOpts::default()
        };
        let result = wait_for_tx(&node, &TxHash::zero(), opts).await.unwrap();
        assert_eq!(result.status, TxStatus::Checkpointed);
    }

    #[tokio::test]
    async fn wait_for_tx_fails_dropped_after_grace_period() {
        let dropped = MockNode::make_receipt(TxStatus::Dropped, None);
        let node = MockNode::new_with_receipt_sequence(vec![Ok(dropped)]);
        let opts = WaitOpts {
            timeout: Duration::from_secs(5),
            interval: Duration::from_millis(10),
            ignore_dropped_receipts_for: Duration::ZERO,
            ..WaitOpts::default()
        };
        let result = wait_for_tx(&node, &TxHash::zero(), opts).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Reverted(_)));
    }

    // -- wait_for_proven tests --

    #[tokio::test]
    async fn wait_for_proven_returns_when_proven() {
        let node = MockNode::new_with_proven_block_sequence(vec![Ok(5), Ok(10)]);
        let receipt = TxReceipt {
            block_number: Some(8),
            ..MockNode::make_receipt(TxStatus::Checkpointed, Some(TxExecutionResult::Success))
        };
        let opts = WaitForProvenOpts {
            proven_timeout: Duration::from_secs(5),
            interval: Duration::from_millis(10),
        };
        let result = wait_for_proven(&node, &receipt, opts).await.unwrap();
        assert!(result >= 8);
    }

    #[tokio::test]
    async fn wait_for_proven_times_out() {
        let node = MockNode::new_with_proven_block_sequence(vec![Ok(1)]);
        let receipt = TxReceipt {
            block_number: Some(100),
            ..MockNode::make_receipt(TxStatus::Checkpointed, Some(TxExecutionResult::Success))
        };
        let opts = WaitForProvenOpts {
            proven_timeout: Duration::from_millis(50),
            interval: Duration::from_millis(100),
        };
        let result = wait_for_proven(&node, &receipt, opts).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Timeout(_)));
    }

    #[tokio::test]
    async fn wait_for_proven_rejects_receipt_without_block() {
        let node = MockNode::new_with_proven_block_sequence(vec![Ok(10)]);
        let receipt = MockNode::make_receipt(TxStatus::Pending, None);
        let opts = WaitForProvenOpts::default();
        let result = wait_for_proven(&node, &receipt, opts).await;
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("no block_number"),
            "should mention missing block_number"
        );
    }

    // -- create_aztec_node_client --

    #[test]
    fn create_client_does_not_panic() {
        let _client = create_aztec_node_client("http://localhost:8080");
    }

    // -- Trait object safety --

    #[test]
    fn aztec_node_is_object_safe() {
        fn _assert_object_safe(_: &dyn AztecNode) {}
    }
}
