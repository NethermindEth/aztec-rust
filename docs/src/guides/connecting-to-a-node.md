# Connecting to a Node

Open a JSON-RPC connection to an Aztec node and verify health.

## Basic Connection

```rust
use aztec_rs::node::{create_aztec_node_client, wait_for_node, AztecNode};

# async fn run() -> Result<(), aztec_rs::Error> {
let node = create_aztec_node_client("http://localhost:8080");
let info = wait_for_node(&node).await?;
println!("node v{}", info.node_version);
# Ok(()) }
```

`wait_for_node` polls until the node responds and returns `NodeInfo`.

## Overriding the URL

The built-in examples read `AZTEC_NODE_URL` from the environment.
You can follow the same pattern in your own code:

```rust
let url = std::env::var("AZTEC_NODE_URL")
    .unwrap_or_else(|_| "http://localhost:8080".to_string());
```

## Querying State

```rust,no_run
# use aztec_rs::node::{create_aztec_node_client, AztecNode};
# async fn run() -> Result<(), aztec_rs::Error> {
# let node = create_aztec_node_client("http://localhost:8080");
let block = node.get_block_number().await?;
# let _ = block; Ok(()) }
```

## Full Runnable Example

Source: [`examples/node_info.rs`](https://github.com/NethermindEth/aztec-rs/blob/main/examples/node_info.rs).

```rust,ignore
{{#include ../../../examples/node_info.rs}}
```

## Next

- [Embedded Wallet Setup](./embedded-wallet-setup.md)
- [`aztec-node-client` reference](../reference/aztec-node-client.md)
