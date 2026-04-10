//! Block header synchronization with the Aztec node.

use aztec_core::error::Error;
use aztec_node_client::AztecNode;

/// Synchronizes the local block header with the node.
pub struct BlockSynchronizer;

impl BlockSynchronizer {
    /// Fetch the latest block header from the node.
    pub async fn sync_block_header<N: AztecNode>(node: &N) -> Result<serde_json::Value, Error> {
        // block_number 0 = latest
        node.get_block_header(0).await
    }

    /// Fetch a specific block header by number.
    pub async fn get_block_header<N: AztecNode>(
        node: &N,
        block_number: u64,
    ) -> Result<serde_json::Value, Error> {
        node.get_block_header(block_number).await
    }
}
