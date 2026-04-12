//! L1 (Ethereum) client for interacting with Aztec protocol contracts.
//!
//! Uses raw JSON-RPC via `reqwest` to avoid heavy alloy dependencies.
//! Provides wrappers for the Inbox contract's `sendL2Message` function.

use aztec_core::types::{AztecAddress, Fr};
use sha3::{Digest, Keccak256};

// ---------------------------------------------------------------------------
// L1 Contract Addresses
// ---------------------------------------------------------------------------

/// Parsed L1 contract addresses from node info.
#[derive(Clone, Debug)]
pub struct L1ContractAddresses {
    pub inbox: String,
    pub outbox: String,
    pub rollup: String,
    pub fee_juice_portal: String,
}

impl L1ContractAddresses {
    /// Parse from the `l1ContractAddresses` JSON in `NodeInfo`.
    pub fn from_json(json: &serde_json::Value) -> Option<Self> {
        Some(Self {
            inbox: json.get("inboxAddress")?.as_str()?.to_owned(),
            outbox: json.get("outboxAddress")?.as_str()?.to_owned(),
            rollup: json.get("rollupAddress")?.as_str()?.to_owned(),
            fee_juice_portal: json.get("feeJuicePortalAddress")?.as_str()?.to_owned(),
        })
    }
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Result of sending an L1→L2 message via the Inbox contract.
#[derive(Clone, Debug)]
pub struct L1ToL2MessageSentResult {
    /// The message hash from the `MessageSent` event.
    pub msg_hash: Fr,
    /// The global leaf index in the L1→L2 message tree.
    pub global_leaf_index: Fr,
    /// The L1 transaction hash.
    pub tx_hash: String,
}

// ---------------------------------------------------------------------------
// Ethereum JSON-RPC helpers
// ---------------------------------------------------------------------------

/// A minimal Ethereum JSON-RPC client.
pub struct EthClient {
    url: String,
    client: reqwest::Client,
}

impl EthClient {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            client: reqwest::Client::new(),
        }
    }

    /// Get the default L1 RPC URL from env or fallback.
    pub fn default_url() -> String {
        std::env::var("ETHEREUM_HOST").unwrap_or_else(|_| "http://localhost:8545".to_owned())
    }

    async fn rpc_call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, aztec_core::Error> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1,
        });
        let resp = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .map_err(|e| aztec_core::Error::InvalidData(format!("L1 RPC error: {e}")))?;

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| aztec_core::Error::InvalidData(format!("L1 RPC parse error: {e}")))?;

        if let Some(err) = json.get("error") {
            return Err(aztec_core::Error::InvalidData(format!(
                "L1 RPC error: {}",
                err
            )));
        }

        Ok(json["result"].clone())
    }

    /// Get the first account from the L1 node (for sandbox use).
    pub async fn get_account(&self) -> Result<String, aztec_core::Error> {
        let result = self.rpc_call("eth_accounts", serde_json::json!([])).await?;
        result
            .as_array()
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned())
            .ok_or_else(|| aztec_core::Error::InvalidData("no L1 accounts available".into()))
    }

    /// Send a raw transaction via `eth_sendTransaction` (sandbox/Anvil only).
    pub async fn send_transaction(
        &self,
        to: &str,
        data: &str,
        from: &str,
    ) -> Result<String, aztec_core::Error> {
        let result = self
            .rpc_call(
                "eth_sendTransaction",
                serde_json::json!([{
                    "from": from,
                    "to": to,
                    "data": data,
                    "gas": "0xf4240", // 1_000_000
                }]),
            )
            .await?;

        result
            .as_str()
            .map(|s| s.to_owned())
            .ok_or_else(|| aztec_core::Error::InvalidData("no tx hash in response".into()))
    }

    /// Wait for a transaction receipt.
    pub async fn wait_for_receipt(
        &self,
        tx_hash: &str,
    ) -> Result<serde_json::Value, aztec_core::Error> {
        for _ in 0..60 {
            let result = self
                .rpc_call("eth_getTransactionReceipt", serde_json::json!([tx_hash]))
                .await?;
            if !result.is_null() {
                return Ok(result);
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        Err(aztec_core::Error::Timeout(
            "L1 tx receipt not available after 30s".into(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Inbox interaction
// ---------------------------------------------------------------------------

/// Compute the `sendL2Message` ABI-encoded calldata.
///
/// Function signature: `sendL2Message((bytes32,uint256),bytes32,bytes32)`
/// Selector: first 4 bytes of keccak256 of the signature.
fn encode_send_l2_message(
    recipient: &AztecAddress,
    rollup_version: u64,
    content: &Fr,
    secret_hash: &Fr,
) -> String {
    // Compute function selector
    let sig = b"sendL2Message((bytes32,uint256),bytes32,bytes32)";
    let mut hasher = Keccak256::new();
    hasher.update(sig);
    let selector = &hasher.finalize()[..4];

    // ABI encode parameters:
    // - recipient.actor (bytes32): 32 bytes
    // - recipient.version (uint256): 32 bytes
    // - content (bytes32): 32 bytes
    // - secretHash (bytes32): 32 bytes
    let actor_bytes = recipient.0.to_be_bytes();
    let mut version_bytes = [0u8; 32];
    version_bytes[24..32].copy_from_slice(&rollup_version.to_be_bytes());
    let content_bytes = content.to_be_bytes();
    let secret_hash_bytes = secret_hash.to_be_bytes();

    let mut calldata = Vec::with_capacity(4 + 128);
    calldata.extend_from_slice(selector);
    calldata.extend_from_slice(&actor_bytes);
    calldata.extend_from_slice(&version_bytes);
    calldata.extend_from_slice(&content_bytes);
    calldata.extend_from_slice(&secret_hash_bytes);

    format!("0x{}", hex::encode(&calldata))
}

/// Send an L1→L2 message via the Inbox contract.
///
/// Returns the message hash and global leaf index from the `MessageSent` event.
pub async fn send_l1_to_l2_message(
    eth_client: &EthClient,
    inbox_address: &str,
    recipient: &AztecAddress,
    rollup_version: u64,
    content: &Fr,
    secret_hash: &Fr,
) -> Result<L1ToL2MessageSentResult, aztec_core::Error> {
    let from = eth_client.get_account().await?;
    let calldata = encode_send_l2_message(recipient, rollup_version, content, secret_hash);

    let tx_hash = eth_client
        .send_transaction(inbox_address, &calldata, &from)
        .await?;

    let receipt = eth_client.wait_for_receipt(&tx_hash).await?;

    // Check status
    let status = receipt
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("0x0");
    if status != "0x1" {
        return Err(aztec_core::Error::InvalidData(format!(
            "L1 tx {tx_hash} failed with status {status}"
        )));
    }

    // Parse MessageSent event from logs
    // Actual Solidity event:
    //   event MessageSent(uint256 indexed checkpointNumber, uint256 index, bytes32 indexed hash, bytes16 rollingHash)
    // Topics: [signature, checkpointNumber, hash]
    // Data: [index (uint256), rollingHash (bytes16 padded to 32)]
    let event_sig = {
        let mut hasher = Keccak256::new();
        hasher.update(b"MessageSent(uint256,uint256,bytes32,bytes16)");
        format!("0x{}", hex::encode(hasher.finalize()))
    };

    let logs = receipt
        .get("logs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| aztec_core::Error::InvalidData("no logs in L1 receipt".into()))?;

    for log in logs {
        let topics = log
            .get("topics")
            .and_then(|v| v.as_array())
            .unwrap_or(&Vec::new())
            .clone();
        if topics.len() >= 3 {
            let topic0 = topics[0].as_str().unwrap_or("");
            if topic0 == event_sig {
                // topics[1] = checkpointNumber (indexed), topics[2] = hash (indexed)
                // data = ABI-encoded (uint256 index, bytes16 rollingHash)
                let hash_hex = topics[2].as_str().unwrap_or("0x0");
                let hash_bytes = hex::decode(hash_hex.strip_prefix("0x").unwrap_or(hash_hex))
                    .unwrap_or_default();

                let mut hsh = [0u8; 32];
                let start = 32usize.saturating_sub(hash_bytes.len());
                hsh[start..].copy_from_slice(&hash_bytes);

                // Parse index from data (first 32 bytes of log data)
                let data_hex = log.get("data").and_then(|v| v.as_str()).unwrap_or("0x");
                let data_bytes = hex::decode(data_hex.strip_prefix("0x").unwrap_or(data_hex))
                    .unwrap_or_default();
                let mut idx = [0u8; 32];
                if data_bytes.len() >= 32 {
                    idx.copy_from_slice(&data_bytes[..32]);
                }

                return Ok(L1ToL2MessageSentResult {
                    msg_hash: Fr::from(hsh),
                    global_leaf_index: Fr::from(idx),
                    tx_hash,
                });
            }
        }
    }

    Err(aztec_core::Error::InvalidData(
        "no MessageSent event found in L1 receipt".into(),
    ))
}
