# aztec-rs

Rust runtime and client workspace for the [Aztec Network](https://aztec.network).
This repository includes an in-process PXE runtime, wallet and account layers,
node and Ethereum clients, contract tooling, and a top-level `aztec-rs` crate
that re-exports the full stack.

This is not just a thin RPC client around node methods. For Aztec v4.x, PXE
runs client-side, and this workspace includes that runtime in `aztec-pxe`.

> **Status:** Active development (v0.5.1). APIs may still change.
>
> **Not yet on crates.io** — depends on noir `1.0.0-beta.18` which is only available via git.
> Install from GitHub as shown below.

## What It Includes

- **Embedded PXE runtime** — in-process private execution engine with note discovery, local stores, kernel simulation/proving, and block sync
- **PXE client surface** — `aztec-pxe-client` defines the PXE trait and shared request/response types used by wallets and runtimes
- **Wallet runtime** — `BaseWallet` composes a PXE backend, Aztec node client, and account provider into a production wallet
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

`aztec-rs` is not yet published to crates.io because it depends on noir crates
(`acvm`, `brillig_vm`, `acir`, `noirc_abi`, `bn254_blackbox_solver`) at
`1.0.0-beta.18`, which are only available as pre-release builds from the
[noir](https://github.com/noir-lang/noir) repository. This matches the same
noir version pinned by upstream
[aztec-packages](https://github.com/AztecProtocol/aztec-packages) (commit
`2db78f8894936db05c53430f364360ac9cc5c61f`).

Add `aztec-rs` as a git dependency in your `Cargo.toml`:

```toml
[dependencies]
aztec-rs = { git = "https://github.com/NethermindEth/aztec-rs.git", tag = "v0.5.1" }
```

The noir git patches are declared in the workspace `Cargo.toml` and apply
automatically when Cargo resolves the dependency — no `[patch.crates-io]`
needed in your project. Once noir publishes stable releases to crates.io,
`aztec-rs` will be published there too and a simple version dependency will
work.

If you only want a subset of the stack, the workspace is also split into
dedicated crates such as `aztec-pxe`, `aztec-pxe-client`, `aztec-wallet`,
`aztec-node-client`, and `aztec-contract`.

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

For Aztec v4.x applications, the usual production entrypoint is
`aztec_rs::wallet::create_embedded_wallet`, which creates a wallet backed by an
in-process PXE runtime and a single node URL. There is no separate PXE server
to configure in that flow.

## Workspace Crates

| Crate | Purpose |
|------|---------|
| `aztec-rs` | Umbrella crate that re-exports the workspace as one dependency |
| `aztec-core` | Core types, ABI support, hashes, fees, errors, and transaction types |
| `aztec-rpc` | JSON-RPC transport layer used by clients |
| `aztec-crypto` | Key derivation, Schnorr, Pedersen, Grumpkin, and related primitives |
| `aztec-node-client` | HTTP client and polling helpers for Aztec node RPC |
| `aztec-pxe-client` | PXE trait plus shared types for simulation, proving, events, and sync |
| `aztec-pxe` | Embedded PXE runtime with local stores, execution, kernel, and sync services |
| `aztec-wallet` | `BaseWallet` and account-provider integration on top of PXE + node backends |
| `aztec-contract` | Contract handles, deployments, authwits, and event decoding |
| `aztec-account` | Account abstraction flows, entrypoints, and account deployment helpers |
| `aztec-fee` | Fee payment strategies and fee-related types |
| `aztec-ethereum` | L1 client and L1<->L2 messaging helpers |

## Examples

Run examples with a local Aztec node (defaults to `http://localhost:8080`):

```bash
# Connect to a node and display info
cargo run --example node_info

# Create a minimal embedded wallet/PXE against the local network
cargo run --example wallet_minimal

# Deploy a contract and verify wallet/node state
cargo run --example deploy_contract

# Private token transfer with PXE-backed note discovery
cargo run --example private_token_transfer

# Compare simulate/profile/send for the same call
cargo run --example simulate_profile_send

# Emit and query public + private events
cargo run --example event_logs

# Deploy a fresh Schnorr account
cargo run --example account_deploy
```

These are the core onboarding examples. The full example inventory and implementation notes live in `examples/ROADMAP.md`, including:

- PXE-focused examples such as `scope_isolation`, `two_pxes`, and `note_getter`
- contract and wallet examples such as `authwit`, `deploy_options`, `public_storage`, and `contract_update`
- fee and cross-chain examples such as `fee_native`, `fee_sponsored`, `fee_juice_claim`, `l1_to_l2_message`, and `l2_to_l1_message`

All examples are intended to run against `aztec start --local-network`. The cross-chain examples assume the local L1 side of that network is available, and `fee_sponsored` uses the vendored `SponsoredFPC` artifact under `fixtures/`.

Override the node URL with the `AZTEC_NODE_URL` environment variable:

```bash
AZTEC_NODE_URL=http://localhost:9090 cargo run --example node_info
```

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
