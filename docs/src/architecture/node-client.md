# Node Client

`aztec-node-client` provides the typed async surface for the Aztec node's JSON-RPC API, plus readiness and receipt polling helpers.

## Context

Every client-side component eventually needs to:

- Ask the node for chain state (blocks, headers, public storage, tree witnesses).
- Submit proven transactions.
- Read public logs.
- Wait for a tx to reach a specific lifecycle status.

All of these go through this crate.

## Design

Two layers:

1. **`AztecNode` trait** — the async interface application code and higher crates depend on.
2. **`HttpNodeClient`** — the concrete RPC-backed implementation returned by `create_aztec_node_client(url)`.

Transport is delegated to [`aztec-rpc`](../reference/aztec-rpc.md)'s `RpcTransport`; request/response types live in [`aztec-core`](../reference/aztec-core.md) so they can be shared with PXE and wallet layers without a cyclic dependency.

## Implementation

Key types:

- `HttpNodeClient` — the concrete implementation.
- `NodeInfo`, `PublicLogFilter`, `PublicLogsResponse`, `PublicLog`, `PublicLogEntry`, `PublicLogBody`, `PublicLogId`, `LogId`, `TxValidationResult`.
- `WaitOpts` — configures `wait_for_tx` (timeout, interval, target status, revert handling, dropped-receipt race window).
- `WaitForProvenOpts` — configures `wait_for_proven`.

Readiness helpers:

- `wait_for_node(&node)` — polls `get_node_info` until the node responds.
- `wait_for_tx(&node, tx_hash, opts)` — polls receipts until the configured `TxStatus` is reached.
- `wait_for_proven(&node, opts)` — polls until the proven block number advances.

## Edge Cases

- **Transient dropped state**: `WaitOpts::ignore_dropped_receipts_for` (default 5 s) prevents returning failure during the normal race between mempool eviction and block inclusion.
- **Revert handling**: `dont_throw_on_revert` lets callers receive a `TxReceipt` for reverted txs instead of an `Error::Reverted`.
- **`send_tx` and `simulate_public_calls`** take `serde_json::Value` by design — the node evolves its tx envelope faster than typed bindings can track it; typed shapes live one layer up.

## Security Considerations

- The node is *untrusted*: it serves chain state but cannot produce valid private proofs, and its responses are constrained by the PXE's kernel verification. Treat `get_public_*` results accordingly; they reflect the node's view of current state.
- No authentication is performed at this layer; if you need it, wrap `HttpNodeClient` or implement `AztecNode` yourself.

## References

- [`aztec-node-client` reference](../reference/aztec-node-client.md)
- [`aztec-rpc` reference](../reference/aztec-rpc.md)
- [PXE Runtime](./pxe-runtime.md) — primary consumer of witness / log methods.
