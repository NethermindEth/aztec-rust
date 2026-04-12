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
    /// L1 ERC-20 Fee Juice token address.
    pub fee_juice: Option<String>,
    /// Fee asset handler address (mints Fee Juice on L1 in sandbox).
    pub fee_asset_handler: Option<String>,
}

impl L1ContractAddresses {
    /// Parse from the `l1ContractAddresses` JSON in `NodeInfo`.
    pub fn from_json(json: &serde_json::Value) -> Option<Self> {
        Some(Self {
            inbox: json.get("inboxAddress")?.as_str()?.to_owned(),
            outbox: json.get("outboxAddress")?.as_str()?.to_owned(),
            rollup: json.get("rollupAddress")?.as_str()?.to_owned(),
            fee_juice_portal: json.get("feeJuicePortalAddress")?.as_str()?.to_owned(),
            fee_juice: json
                .get("feeJuiceAddress")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned()),
            fee_asset_handler: json
                .get("feeAssetHandlerAddress")
                .and_then(|v| v.as_str())
                .map(|s| s.to_owned()),
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

    pub async fn rpc_call(
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

// ---------------------------------------------------------------------------
// Fee Juice bridge helpers
// ---------------------------------------------------------------------------

fn keccak_selector(sig: &[u8]) -> [u8; 4] {
    let mut hasher = Keccak256::new();
    hasher.update(sig);
    let hash = hasher.finalize();
    let mut sel = [0u8; 4];
    sel.copy_from_slice(&hash[..4]);
    sel
}

/// ABI-encode `mint(address)` on the FeeAssetHandler.
fn encode_mint(to: &str) -> String {
    let sel = keccak_selector(b"mint(address)");
    let mut calldata = Vec::with_capacity(4 + 32);
    calldata.extend_from_slice(&sel);
    // address is left-padded to 32 bytes
    let addr_bytes = hex::decode(to.strip_prefix("0x").unwrap_or(to)).unwrap_or_default();
    let mut padded = [0u8; 32];
    let start = 32usize.saturating_sub(addr_bytes.len());
    padded[start..].copy_from_slice(&addr_bytes);
    calldata.extend_from_slice(&padded);
    format!("0x{}", hex::encode(&calldata))
}

/// ABI-encode `approve(address,uint256)` on the ERC-20 token.
fn encode_approve(spender: &str, amount: u128) -> String {
    let sel = keccak_selector(b"approve(address,uint256)");
    let mut calldata = Vec::with_capacity(4 + 64);
    calldata.extend_from_slice(&sel);
    let addr_bytes = hex::decode(spender.strip_prefix("0x").unwrap_or(spender)).unwrap_or_default();
    let mut padded = [0u8; 32];
    let start = 32usize.saturating_sub(addr_bytes.len());
    padded[start..].copy_from_slice(&addr_bytes);
    calldata.extend_from_slice(&padded);
    let mut amt = [0u8; 32];
    amt[16..32].copy_from_slice(&amount.to_be_bytes());
    calldata.extend_from_slice(&amt);
    format!("0x{}", hex::encode(&calldata))
}

/// ABI-encode `depositToAztecPublic(bytes32,uint256,bytes32)` on the Fee Juice Portal.
fn encode_deposit_to_aztec_public(to: &AztecAddress, amount: u128, secret_hash: &Fr) -> String {
    let sel = keccak_selector(b"depositToAztecPublic(bytes32,uint256,bytes32)");
    let mut calldata = Vec::with_capacity(4 + 96);
    calldata.extend_from_slice(&sel);
    calldata.extend_from_slice(&to.0.to_be_bytes());
    let mut amt = [0u8; 32];
    amt[16..32].copy_from_slice(&amount.to_be_bytes());
    calldata.extend_from_slice(&amt);
    calldata.extend_from_slice(&secret_hash.to_be_bytes());
    format!("0x{}", hex::encode(&calldata))
}

/// ABI-encode `mintAmount()` view call on the FeeAssetHandler.
///
/// `mintAmount` is a public state variable; Solidity auto-generates a
/// `mintAmount()` getter — there is no `getMintAmount()` function.
fn encode_mint_amount() -> String {
    let sel = keccak_selector(b"mintAmount()");
    format!("0x{}", hex::encode(sel))
}

/// Result of preparing Fee Juice on L1 for an L2 claim.
#[derive(Clone, Debug)]
pub struct FeeJuiceBridgeResult {
    /// Amount bridged.
    pub claim_amount: u128,
    /// Secret to claim with.
    pub claim_secret: Fr,
    /// Index in the L1→L2 message tree.
    pub message_leaf_index: u64,
    /// Message hash (the `key` from the portal event) used to check sync status.
    pub message_hash: Fr,
}

/// Mint Fee Juice on L1 and bridge it to L2 via the Fee Juice Portal.
///
/// Mirrors upstream `GasBridgingTestHarness.prepareTokensOnL1()`:
/// 1. Queries `getMintAmount()` from the FeeAssetHandler
/// 2. Mints ERC-20 tokens on L1
/// 3. Approves the portal to spend them
/// 4. Calls `depositToAztecPublic` on the portal
/// 5. Parses the `DepositToAztecPublic` event for the message leaf index
///
/// Returns the claim data needed for `FeeJuicePaymentMethodWithClaim`.
pub async fn prepare_fee_juice_on_l1(
    eth_client: &EthClient,
    l1_addresses: &L1ContractAddresses,
    recipient: &AztecAddress,
) -> Result<FeeJuiceBridgeResult, aztec_core::Error> {
    let fee_juice_address = l1_addresses
        .fee_juice
        .as_deref()
        .ok_or_else(|| aztec_core::Error::InvalidData("feeJuiceAddress not in node info".into()))?;
    let handler_address = l1_addresses.fee_asset_handler.as_deref().ok_or_else(|| {
        aztec_core::Error::InvalidData("feeAssetHandlerAddress not in node info".into())
    })?;
    let portal_address = &l1_addresses.fee_juice_portal;
    let from = eth_client.get_account().await?;

    // 1. Query mint amount from FeeAssetHandler (public state variable getter)
    let mint_amount = {
        let data = encode_mint_amount();
        let result = eth_client
            .rpc_call(
                "eth_call",
                serde_json::json!([{ "to": handler_address, "data": data }, "latest"]),
            )
            .await
            .map_err(|e| {
                aztec_core::Error::InvalidData(format!("mintAmount() call failed: {e}"))
            })?;
        let hex_str = result.as_str().unwrap_or("0x0");
        let bytes = hex::decode(hex_str.strip_prefix("0x").unwrap_or(hex_str)).unwrap_or_default();
        if bytes.len() >= 32 {
            let mut amt_bytes = [0u8; 16];
            amt_bytes.copy_from_slice(&bytes[16..32]);
            u128::from_be_bytes(amt_bytes)
        } else {
            return Err(aztec_core::Error::InvalidData(format!(
                "mintAmount() returned invalid data ({} bytes)",
                bytes.len()
            )));
        }
    };

    // 2. Mint tokens on L1
    let mint_data = encode_mint(&from);
    let tx_hash = eth_client
        .send_transaction(handler_address, &mint_data, &from)
        .await
        .map_err(|e| aztec_core::Error::InvalidData(format!("mint tx send failed: {e}")))?;
    let receipt = eth_client.wait_for_receipt(&tx_hash).await?;
    let status = receipt
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("0x0");
    if status != "0x1" {
        return Err(aztec_core::Error::InvalidData(format!(
            "mint tx reverted: {status}"
        )));
    }

    // 3. Approve portal to spend tokens
    let approve_data = encode_approve(portal_address, mint_amount);
    let tx_hash = eth_client
        .send_transaction(fee_juice_address, &approve_data, &from)
        .await
        .map_err(|e| aztec_core::Error::InvalidData(format!("approve tx send failed: {e}")))?;
    let receipt = eth_client.wait_for_receipt(&tx_hash).await?;
    let status = receipt
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("0x0");
    if status != "0x1" {
        return Err(aztec_core::Error::InvalidData(format!(
            "approve tx reverted: {status}"
        )));
    }

    // 4. Generate claim secret
    let (claim_secret, claim_secret_hash) = super::messaging::generate_claim_secret();

    // 5. Deposit to Aztec public
    let deposit_data = encode_deposit_to_aztec_public(recipient, mint_amount, &claim_secret_hash);
    let tx_hash = eth_client
        .send_transaction(portal_address, &deposit_data, &from)
        .await
        .map_err(|e| {
            aztec_core::Error::InvalidData(format!("depositToAztecPublic tx send failed: {e}"))
        })?;
    let receipt = eth_client.wait_for_receipt(&tx_hash).await?;
    let status = receipt
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("0x0");
    if status != "0x1" {
        return Err(aztec_core::Error::InvalidData(format!(
            "depositToAztecPublic tx reverted: {status}"
        )));
    }

    // 6. Parse the Inbox's MessageSent event from the deposit receipt.
    //
    // The portal calls `inbox.sendL2Message(...)` internally, so the
    // deposit receipt contains both the portal's DepositToAztecPublic
    // event AND the Inbox's MessageSent event.  We parse MessageSent
    // because its hash (topics[2]) is the canonical L1→L2 message hash
    // that `get_l1_to_l2_message_checkpoint` expects.
    //
    //   event MessageSent(uint256 indexed checkpointNumber, uint256 index,
    //                     bytes32 indexed hash, bytes16 rollingHash)
    //   Topics: [sig, checkpointNumber, hash]
    //   Data:   [index (uint256), rollingHash (bytes16 padded to 32)]
    let message_sent_sig = {
        let mut hasher = Keccak256::new();
        hasher.update(b"MessageSent(uint256,uint256,bytes32,bytes16)");
        format!("0x{}", hex::encode(hasher.finalize()))
    };

    let logs = receipt
        .get("logs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            aztec_core::Error::InvalidData("no logs in depositToAztecPublic receipt".into())
        })?;

    for log in logs {
        let topics = log
            .get("topics")
            .and_then(|v| v.as_array())
            .unwrap_or(&Vec::new())
            .clone();
        if topics.len() >= 3 && topics[0].as_str().unwrap_or("") == message_sent_sig {
            // topics[2] = hash (indexed)
            let hash_hex = topics[2].as_str().unwrap_or("0x0");
            let hash_bytes =
                hex::decode(hash_hex.strip_prefix("0x").unwrap_or(hash_hex)).unwrap_or_default();
            let mut msg_hash = [0u8; 32];
            let start = 32usize.saturating_sub(hash_bytes.len());
            msg_hash[start..].copy_from_slice(&hash_bytes);

            // data[0..32] = index (uint256)
            let data_hex = log.get("data").and_then(|v| v.as_str()).unwrap_or("0x");
            let data_bytes =
                hex::decode(data_hex.strip_prefix("0x").unwrap_or(data_hex)).unwrap_or_default();
            let message_leaf_index = if data_bytes.len() >= 32 {
                let mut idx = [0u8; 8];
                idx.copy_from_slice(&data_bytes[24..32]);
                u64::from_be_bytes(idx)
            } else {
                0
            };

            return Ok(FeeJuiceBridgeResult {
                claim_amount: mint_amount,
                claim_secret,
                message_leaf_index,
                message_hash: Fr::from(msg_hash),
            });
        }
    }

    Err(aztec_core::Error::InvalidData(
        "no MessageSent event found in deposit receipt".into(),
    ))
}
