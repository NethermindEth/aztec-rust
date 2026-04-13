# Errors

`aztec-rs` has a single top-level error type, re-exported from [`aztec_core::error::Error`](https://github.com/NethermindEth/aztec-rs/blob/main/crates/core/src/error.rs).
Every crate in the workspace converts its internal failures into this type.

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

## Rustdoc

```bash
cargo doc --open
```

Search for `Error` in the generated docs to see conversion impls per crate.
