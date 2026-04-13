# Quickstart

A five-minute tour.
By the end of this page you will have verified your setup against a local Aztec node and know where to go next for wallets, contracts, and accounts.

The detail pages are linked inline; follow them when you need more than the tour covers.

## 0. Prerequisites

- Rust toolchain (edition 2021 or later) — see [Installation](./guides/installation.md) for the full list.
- A running Aztec node reachable over HTTP (default `http://localhost:8080`).
  See the [Aztec docs](https://docs.aztec.network) for sandbox setup.

## 1. Add the Dependency

```toml
[dependencies]
aztec-rs = { git = "https://github.com/NethermindEth/aztec-rs.git", tag = "v0.5.1" }
tokio   = { version = "1", features = ["full"] }
```

Full instructions (including subset crates) live in [Installation](./guides/installation.md).

## 2. Talk to a Node

```rust,ignore
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

If `wait_for_node` returns, your setup is good.
See [Connecting to a Node](./guides/connecting-to-a-node.md) for richer query examples.

## 3. Build a Wallet

For Aztec v4.x applications the canonical entrypoint is `aztec_rs::wallet::create_embedded_wallet`.
It wires up an in-process PXE, a node client, and an account provider behind one object.

Walk through the full flow in [Embedded Wallet Setup](./guides/embedded-wallet-setup.md).

## 4. Run a Shipped Example

The repository ships end-to-end examples that cover the common flows:

```bash
# Connect + inspect node info
cargo run --example node_info

# Minimal wallet + chain info
cargo run --example wallet_minimal

# Deploy a contract from the bundled fixtures
cargo run --example deploy_contract

# Simulate → profile → send a single call
cargo run --example simulate_profile_send

# Full account lifecycle: keys → deploy → first tx
cargo run --example account_deploy
```

Override the node URL with `AZTEC_NODE_URL`:

```bash
AZTEC_NODE_URL=http://localhost:9090 cargo run --example node_info
```

A full list of examples lives in the repository's `examples/` directory — each is referenced from the matching guide page.

## Next Steps

| Goal                                    | Go to                                                       |
| --------------------------------------- | ----------------------------------------------------------- |
| Understand the runtime model            | [Concepts Overview](./concepts/overview.md)                 |
| See how the crates fit together         | [Architecture Overview](./architecture/overview.md)         |
| Deploy + call contracts                 | [Deploying Contracts](./guides/deploying-contracts.md), [Calling Contracts](./guides/calling-contracts.md) |
| Set up accounts and fees                | [Account Lifecycle](./guides/account-lifecycle.md), [Fee Payments](./guides/fee-payments.md) |
| Read or write across L1 ↔ L2            | [Cross-Chain Messaging](./guides/cross-chain-messaging.md)  |
| Browse the typed API                    | [Crate Index](./reference/crates.md)                        |
