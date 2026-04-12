//! Cross-chain utilities for L1↔L2 message readiness checking.
//!
//! Mirrors upstream `aztec.js/src/utils/cross_chain.ts`.

use aztec_core::types::Fr;
use aztec_node_client::node::AztecNode;
use std::time::{Duration, Instant};

/// Check whether an L1-to-L2 message is ready for consumption on L2.
///
/// A message is ready when its checkpoint number is ≤ the latest block's
/// checkpoint number.
///
/// Mirrors TS `isL1ToL2MessageReady(node, l1ToL2MessageHash)`.
pub async fn is_l1_to_l2_message_ready<N: AztecNode>(
    node: &N,
    l1_to_l2_message_hash: &Fr,
) -> Result<bool, aztec_core::Error> {
    let checkpoint = node
        .get_l1_to_l2_message_checkpoint(l1_to_l2_message_hash)
        .await?;
    let Some(msg_checkpoint) = checkpoint else {
        return Ok(false);
    };

    let block_number = node.get_block_number().await?;
    if block_number == 0 {
        return Ok(false);
    }
    let block = node.get_block(block_number).await?;
    let Some(block_json) = block else {
        return Ok(false);
    };

    // Extract checkpoint number from block header
    let block_checkpoint = block_json
        .pointer("/header/globalVariables/blockNumber")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    Ok(msg_checkpoint <= block_checkpoint)
}

/// Wait until an L1-to-L2 message is ready for consumption, with a timeout.
///
/// Polls every second until the message is ready or the timeout expires.
///
/// Mirrors TS `waitForL1ToL2MessageReady(node, hash, opts)`.
pub async fn wait_for_l1_to_l2_message_ready<N: AztecNode>(
    node: &N,
    l1_to_l2_message_hash: &Fr,
    timeout: Duration,
) -> Result<(), aztec_core::Error> {
    let start = Instant::now();
    loop {
        if is_l1_to_l2_message_ready(node, l1_to_l2_message_hash).await? {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            return Err(aztec_core::Error::Timeout(format!(
                "L1-to-L2 message {} not ready within {:?}",
                l1_to_l2_message_hash, timeout
            )));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
