# Errors

`aztec-rs` has a single top-level error type, re-exported from [`aztec_core::error::Error`](https://github.com/NethermindEth/aztec-rs/blob/main/crates/core/src/error.rs).
Every crate in the workspace converts its internal failures into this type.

## Start From User Tasks

| Symptom | Likely variant | What to check |
| ------- | -------------- | ------------- |
| Node URL is wrong or offline | `Transport` | `AZTEC_NODE_URL`, sandbox status, network connectivity |
| Node returns a JSON-RPC error | `Rpc` | Method name, parameters, node logs |
| Artifact or call arguments do not match | `Abi` | Function name, argument count, ABI type |
| Hex, address, or field value will not parse | `InvalidData` | Input format and `0x` prefix |
| Transaction simulation or inclusion failed | `Reverted` | Contract assertion message and authwit / fee setup |
| A poller never reached the target state | `Timeout` | Node progress, block production, `WaitOpts` timeout |

## The `Error` Enum

```rust,ignore
pub enum Error {
    Transport(String),           // HTTP / network failure
    Json(String),                // Serde JSON failure
    Abi(String),                 // ABI or artifact validation failure
    InvalidData(String),         // Invalid or unexpected data
    Rpc { code: i64, message: String }, // JSON-RPC server error
    Reverted(String),            // Tx execution reverted
    Timeout(String),             // Operation timed out
}
```

## Conversions

Built-in `From` impls collapse common lower-level errors into `Error`:

- `reqwest::Error` → `Error::Transport`
- `serde_json::Error` → `Error::Json`
- `url::ParseError` → `Error::Transport`

Most crate-local errors also implement `From<_>` for `Error`; application code typically only sees `aztec_rs::Error`.

## Variant Reference

| Variant         | Typical source                                         |
| --------------- | ------------------------------------------------------ |
| `Transport`     | `aztec-rpc`, `aztec-node-client`, `aztec-ethereum`     |
| `Json`          | Any layer decoding RPC responses or artifacts          |
| `Abi`           | `aztec-core::abi`, `aztec-contract`                    |
| `InvalidData`   | Field parsing, address parsing, hex decoding           |
| `Rpc`           | Node returns a non-success JSON-RPC envelope           |
| `Reverted`      | Simulation or inclusion revert from a public call      |
| `Timeout`       | Readiness pollers (`wait_for_node`, `wait_for_tx`, …)  |

## Pattern Matching

```rust,ignore
match result {
    Err(aztec_rs::Error::Reverted(msg)) => eprintln!("tx reverted: {msg}"),
    Err(aztec_rs::Error::Rpc { code, message }) => eprintln!("rpc {code}: {message}"),
    Err(e) => eprintln!("other error: {e}"),
    Ok(v) => v,
}
```

For CLI-style tools, keep the top-level result simple and handle only the variants where you can give the user a better next step:

```rust,ignore
if let Err(err) = run().await {
    match err {
        aztec_rs::Error::Transport(msg) => eprintln!("cannot reach node: {msg}"),
        aztec_rs::Error::Reverted(msg) => eprintln!("transaction reverted: {msg}"),
        other => eprintln!("{other}"),
    }
}
```

## Rustdoc

```bash
cargo doc --open
```

Search for `Error` in the generated docs to see conversion impls per crate.
