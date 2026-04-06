# aztec-rs

Rust SDK for the [Aztec Network](https://aztec.network). Provides a client library for interacting with Aztec nodes, managing wallets and accounts, deploying contracts, and sending transactions. The design mirrors the upstream [`aztec.js`](https://github.com/AztecProtocol/aztec-packages/tree/master/yarn-project/aztec.js) package.

> **Status:** Early development (v0.1.0). APIs are subject to change.

## Features

- **Node client** — connect to an Aztec node over JSON-RPC, query blocks, chain info, and wait for readiness
- **Contract interaction** — load contract artifacts, build and send function calls (private, public, utility)
- **Contract deployment** — builder-pattern deployer with deterministic addressing and multi-phase deployment
- **Account abstraction** — traits and manager for account lifecycle, entrypoint execution, and authorization witnesses
- **Transaction handling** — construct, simulate, send, and track transactions through their full lifecycle
- **Event decoding** — filter and decode public and private contract events
- **Type system** — BN254 field elements, Aztec addresses, keys, and contract instances

## Installation

Add `aztec-rs` to your `Cargo.toml`:

```toml
[dependencies]
aztec-rs = "0.1.1"
```

## Quick Start

```rust
use aztec_rs::node::{create_aztec_node_client, wait_for_node, AztecNode};

#[tokio::main]
async fn main() -> Result<(), aztec_rs::Error> {
    let node = create_aztec_node_client("http://localhost:8080");
    let info = wait_for_node(&node).await?;
    println!("Connected to node v{}", info.node_version);

    let block = node.get_block_number().await?;
    println!("Current block: {block}");
    Ok(())
}
```

## Examples

Run examples with a local Aztec node (defaults to `http://localhost:8080`):

```bash
# Connect to a node and display info
cargo run --example node_info

# Make a contract function call
cargo run --example contract_call

# Deploy a contract
cargo run --example deploy_contract

# Full account lifecycle
cargo run --example account_flow
```

Override the node URL with the `AZTEC_NODE_URL` environment variable:

```bash
AZTEC_NODE_URL=http://localhost:9090 cargo run --example node_info
```

## Modules

| Module | Description |
|--------|-------------|
| `abi` | ABI types, selectors, and contract artifact loading |
| `account` | Account abstraction traits, manager, and deployment |
| `authorization` | Authorization witness types and helpers |
| `contract` | Contract handles and function interactions |
| `deployment` | Contract deployment helpers and deployer builder |
| `events` | Public and private event types and decoding |
| `fee` | Gas and fee payment types |
| `messaging` | L1-L2 messaging helpers |
| `node` | Node client, readiness polling, and receipt waiting |
| `tx` | Transaction types, receipts, statuses, and execution payloads |
| `types` | Core field (Fr), address, key, and contract instance types |
| `wallet` | Wallet trait and mock implementation |

## Development

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (edition 2021)
- A running [Aztec node](https://docs.aztec.network) for integration tests and examples

### Build

```bash
cargo build
```

### Test

```bash
# Unit tests
cargo test

# Integration tests (requires a running Aztec node)
cargo test --test integration -- --ignored
```

### Lint

The project ships a strict Clippy configuration via a `cargo lint` alias:

```bash
cargo lint
```

### Documentation

```bash
cargo doc --open
```

### Format

```bash
cargo fmt
```

## License

Licensed under the [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0).
