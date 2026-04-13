# `aztec-node-client`

Typed async client for the Aztec node's JSON-RPC surface, plus readiness polling for nodes and transactions.

Source: `crates/node-client/src/`.

## Entry Points

```rust,no_run
use aztec_rs::node::{create_aztec_node_client, wait_for_node, AztecNode};

# async fn example() -> Result<(), aztec_rs::Error> {
let node = create_aztec_node_client("http://localhost:8080");
let info = wait_for_node(&node).await?;
let block = node.get_block_number().await?;
# let _ = (info, block); Ok(())
# }
```

- `create_aztec_node_client(url)` — returns an `HttpNodeClient`.
- `wait_for_node(&node)` — polls `get_node_info` until it succeeds; returns the first `NodeInfo`.
- `wait_for_tx(&node, tx_hash, opts)` — polls until `TxReceipt` reaches the configured status.
- `wait_for_proven(&node, opts)` — polls until the proven block number advances per `WaitForProvenOpts`.

## The `AztecNode` Trait

```rust,ignore
#[async_trait]
pub trait AztecNode: Send + Sync {
    // State
    async fn get_node_info(&self) -> Result<NodeInfo, Error>;
    async fn get_block_number(&self) -> Result<u64, Error>;
    async fn get_proven_block_number(&self) -> Result<u64, Error>;
    async fn get_block_header(&self, block_number: u64) -> Result<serde_json::Value, Error>;
    async fn get_block(&self, block_number: u64) -> Result<Option<serde_json::Value>, Error>;

    // Transactions
    async fn send_tx(&self, tx: &serde_json::Value) -> Result<(), Error>;
    async fn get_tx_receipt(&self, tx_hash: &TxHash) -> Result<TxReceipt, Error>;
    async fn get_tx_effect(&self, tx_hash: &TxHash) -> Result<Option<serde_json::Value>, Error>;
    async fn get_tx_by_hash(&self, tx_hash: &TxHash) -> Result<Option<serde_json::Value>, Error>;
    async fn simulate_public_calls(&self, tx: &serde_json::Value, skip_fee_enforcement: bool) -> Result<serde_json::Value, Error>;
    async fn is_valid_tx(&self, tx: &serde_json::Value) -> Result<TxValidationResult, Error>;

    // Contracts
    async fn get_contract(&self, address: &AztecAddress) -> Result<Option<ContractInstanceWithAddress>, Error>;
    async fn get_contract_class(&self, id: &Fr) -> Result<Option<serde_json::Value>, Error>;

    // Tree witnesses (PXE simulation + proving)
    async fn get_note_hash_membership_witness(&self, block: u64, hash: &Fr) -> Result<Option<serde_json::Value>, Error>;
    async fn get_nullifier_membership_witness(&self, block: u64, nullifier: &Fr) -> Result<Option<serde_json::Value>, Error>;
    async fn get_low_nullifier_membership_witness(&self, block: u64, nullifier: &Fr) -> Result<Option<serde_json::Value>, Error>;
    async fn get_public_data_witness(&self, block: u64, slot: &Fr) -> Result<Option<serde_json::Value>, Error>;
    async fn get_public_storage_at(&self, block: u64, contract: &AztecAddress, slot: &Fr) -> Result<Fr, Error>;
    async fn get_l1_to_l2_message_membership_witness(&self, block: u64, entry_key: &Fr) -> Result<Option<serde_json::Value>, Error>;
    async fn get_l1_to_l2_message_checkpoint(&self, message: &Fr) -> Result<Option<u64>, Error>;
    async fn get_block_hash_membership_witness(&self, block: u64, hash: &Fr) -> Result<Option<serde_json::Value>, Error>;
    async fn find_leaves_indexes(&self, block: u64, tree_id: &str, leaves: &[Fr]) -> Result<Vec<Option<u64>>, Error>;

    // Logs
    async fn get_public_logs(&self, filter: PublicLogFilter) -> Result<PublicLogsResponse, Error>;
    async fn get_private_logs_by_tags(&self, tags: &[Fr]) -> Result<serde_json::Value, Error>;
    async fn get_public_logs_by_tags_from_contract(&self, contract: &AztecAddress, tags: &[Fr]) -> Result<serde_json::Value, Error>;

    // Debugging
    async fn register_contract_function_signatures(&self, signatures: &[String]) -> Result<(), Error>;
}
```

## Key Types

| Type                      | Purpose                                                                |
| ------------------------- | ---------------------------------------------------------------------- |
| `HttpNodeClient`          | The concrete RPC-backed implementation returned by `create_aztec_node_client` |
| `NodeInfo`                | Node version + protocol metadata                                       |
| `PublicLogFilter`         | Block range + contract + event-selector filter                         |
| `PublicLogsResponse`, `PublicLog`, `PublicLogEntry`, `PublicLogBody`, `PublicLogId`, `LogId` | Event-log response shapes             |
| `TxValidationResult`      | Result of `is_valid_tx`                                                |
| `WaitOpts`                | Tunables for `wait_for_tx` (timeout, interval, target status, revert handling) |
| `WaitForProvenOpts`       | Tunables for `wait_for_proven`                                         |

### `WaitOpts` Defaults

- `timeout`: 300 s
- `interval`: 1 s
- `wait_for_status`: `TxStatus::Checkpointed`
- `dont_throw_on_revert`: false
- `ignore_dropped_receipts_for`: 5 s (avoids spurious `Dropped` races between mempool and inclusion)

## Typical Use

Application code depends on the `AztecNode` trait rather than `HttpNodeClient`, so alternate implementations (mocks, caching proxies) can be slotted in.
The PXE sync loop is the heaviest consumer of the witness / log methods.

## Full API

Bundled rustdoc: [`api/aztec_node_client/`](../api/aztec_node_client/index.html).
Local regeneration:

```bash
cargo doc -p aztec-node-client --open
```

## See Also

- [`aztec-rpc`](./aztec-rpc.md) — underlying transport.
- [`aztec-pxe`](./aztec-pxe.md) — primary consumer of the witness / simulation methods.
