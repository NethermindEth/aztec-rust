# aztec-rs

Rust SDK for the [Aztec Network](https://aztec.network). Provides a client library for interacting with Aztec nodes, managing wallets and accounts, deploying contracts, and sending transactions. The design mirrors the upstream [`aztec.js`](https://github.com/AztecProtocol/aztec-packages/tree/master/yarn-project/aztec.js) package.

> **Status:** Active development (v0.4.0). APIs may still change.

## Features

- **Embedded PXE** — in-process private execution engine with note discovery, kernel proving, and block sync
- **Node client** — connect to an Aztec node over JSON-RPC, query blocks, chain info, and wait for readiness
- **Contract interaction** — load contract artifacts, build and send function calls (private, public, utility)
- **Contract deployment** — builder-pattern deployer with deterministic addressing, class registration, and instance publication
- **Account abstraction** — Schnorr/ECDSA/SingleKey account flavors, entrypoint execution, and authorization witnesses
- **Auth witnesses** — create, validate, and consume authorization witnesses in private and public contexts
- **Fee payments** — native, sponsored, private FPC, and Fee Juice claim-based payment methods
- **Cross-chain messaging** — L1-to-L2 and L2-to-L1 message sending, readiness polling, and consumption
- **Cryptography** — BN254/Grumpkin field arithmetic, Poseidon2, Pedersen, Schnorr signing, key derivation
- **Transaction handling** — construct, simulate, send, and track transactions through their full lifecycle
- **Event decoding** — filter and decode public and private contract events
- **Type system** — BN254 field elements, Aztec addresses, keys, and contract instances

## Installation

Add `aztec-rs` to your `Cargo.toml`:

```toml
[dependencies]
aztec-rs = "0.4.0"
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
| `abi` | ABI types, selectors, encoding/decoding, and contract artifact loading |
| `account` | Account abstraction traits, Schnorr/ECDSA/SingleKey flavors, manager, and deployment |
| `authwit` | Authorization witness creation, validation, and public auth registry interaction |
| `contract` | Contract handles, function interactions, and batch calls |
| `cross_chain` | L1-to-L2 message readiness checking and polling utilities |
| `crypto` | Key derivation, Schnorr signing, Pedersen hashing, and Grumpkin curve operations |
| `deployment` | Contract deployment, class registration, and instance publication |
| `events` | Public and private event types and decoding |
| `fee` | Gas settings and fee payment methods (native, sponsored, private FPC, claim-based) |
| `hash` | Poseidon2, SHA-256, authwit hashing, and cross-chain message hashing |
| `l1_client` | Ethereum JSON-RPC client for Inbox/Outbox contract interaction |
| `messaging` | L1-L2 messaging types (L1Actor, L2Actor, L1ToL2Message, claims) |
| `node` | Node client, readiness polling, and receipt waiting |
| `pxe` | Embedded PXE runtime with private execution, note stores, and block sync |
| `tx` | Transaction types, receipts, statuses, and execution payloads |
| `types` | Core field (Fr), address, key, and contract instance types |
| `wallet` | BaseWallet implementation backed by embedded PXE and node connections |

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

# E2E tests (requires a running Aztec sandbox)
AZTEC_NODE_URL=http://localhost:8080 cargo test --test e2e_token_transfer_private -- --ignored --nocapture
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
