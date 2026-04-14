# `aztec-rpc`

JSON-RPC HTTP transport shared by the node client (and internally by other clients that speak HTTP JSON-RPC).

Source: `crates/rpc/src/`.

## Start From User Tasks

Use this crate only when a typed client does not wrap the JSON-RPC method you need yet.
For normal node work, prefer [`aztec-node-client`](./aztec-node-client.md); for L1 helpers, prefer [`aztec-ethereum`](./aztec-ethereum.md).

| Task | Method | Example |
| ---- | ------ | ------- |
| Call a method with a required result | `call::<T>` | `transport.call("node_getNodeInfo", json!([])).await?` |
| Call a method that may return `null` | `call_optional::<T>` | `transport.call_optional("node_getBlock", json!([block])).await?` |
| Call a method where the result is irrelevant | `call_void` | `transport.call_void("node_sendTx", params).await?` |

```rust,ignore
use aztec_rpc::RpcTransport;
use serde_json::json;
use std::time::Duration;

let transport = RpcTransport::new(
    "http://localhost:8080".to_owned(),
    Duration::from_secs(30),
);
let block: u64 = transport.call("node_getBlockNumber", json!([])).await?;
```

## Public Surface

One module: `rpc`, re-exported at the crate root.

### `RpcTransport`

```rust,ignore
pub struct RpcTransport { /* private */ }

impl RpcTransport {
    pub fn new(url: String, timeout: Duration) -> Self;
    pub fn url(&self) -> &str;
    pub fn timeout(&self) -> Duration;

    pub async fn call<T: DeserializeOwned>(
        &self, method: &str, params: serde_json::Value,
    ) -> Result<T, Error>;

    pub async fn call_optional<T: DeserializeOwned>(
        &self, method: &str, params: serde_json::Value,
    ) -> Result<Option<T>, Error>;

    pub async fn call_void(
        &self, method: &str, params: serde_json::Value,
    ) -> Result<(), Error>;
}
```

- `call` — deserializes the result into `T`.
- `call_optional` — returns `Ok(None)` when the server returns `null`.
- `call_void` — discards the result; used for notifications.

All three produce `aztec_core::Error` on transport, JSON, or RPC-level failure.

## Error Handling

The crate re-exports `aztec_core::Error`.
See [Errors](./errors.md) for the unified error taxonomy.

## Typical Use

`RpcTransport` is used internally by [`aztec-node-client`](./aztec-node-client.md) and [`aztec-ethereum`](./aztec-ethereum.md).
Direct use is rare; prefer the typed clients.

## Full API

Bundled rustdoc: [`api/aztec_rpc/`](../api/aztec_rpc/index.html).
Local regeneration:

```bash
cargo doc -p aztec-rpc --open
```
