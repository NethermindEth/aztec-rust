# `aztec-rs` (umbrella crate)

Top-level crate that re-exports the entire workspace.
Depend on this when you want a single dependency; pick individual crates for a slimmer build.

Source: `src/lib.rs`.

## Public Modules

Every module is a curated re-export from one or more workspace crates.

| Module                 | Re-exports from                                    | Purpose                                                |
| ---------------------- | -------------------------------------------------- | ------------------------------------------------------ |
| `aztec_rs::abi`        | `aztec_core::abi`                                  | ABI types, selectors, artifact loading                 |
| `aztec_rs::account`    | `aztec_account::*`                                 | Account abstraction, entrypoints, Schnorr / signerless |
| `aztec_rs::authorization` | `aztec_account::authorization`                  | Authwit types                                          |
| `aztec_rs::authwit`    | `aztec_contract::authwit`                          | Authwit interaction helpers                            |
| `aztec_rs::contract`   | `aztec_contract::contract`                         | Contract handles and function calls                    |
| `aztec_rs::deployment` | `aztec_contract::deployment`                       | Deployer builder                                       |
| `aztec_rs::error`      | `aztec_core::error`                                | `Error` enum and conversions                           |
| `aztec_rs::events`     | `aztec_contract::events`                           | Public + private event decoding                        |
| `aztec_rs::constants`  | `aztec_core::constants`                            | Protocol contract addresses, domain separators         |
| `aztec_rs::crypto`     | `aztec_crypto`                                     | Keys, Schnorr, Pedersen, Grumpkin primitives           |
| `aztec_rs::hash`       | `aztec_core::hash`                                 | Poseidon2 hashing                                      |
| `aztec_rs::fee`        | `aztec_core::fee` + `aztec_fee`                    | Gas types + payment methods                            |
| `aztec_rs::cross_chain`| `aztec_ethereum::cross_chain`                      | L1↔L2 message readiness polling                        |
| `aztec_rs::l1_client`  | `aztec_ethereum::l1_client`                        | L1 RPC + Inbox/Outbox                                  |
| `aztec_rs::messaging`  | `aztec_ethereum::messaging`                        | L1↔L2 message construction                             |
| `aztec_rs::node`       | `aztec_node_client::node`                          | `AztecNode`, readiness polling, receipts               |
| `aztec_rs::pxe`        | `aztec_pxe_client::pxe`                            | `Pxe` trait, readiness, request/response types         |
| `aztec_rs::embedded_pxe`| `aztec_pxe::*`                                    | Embedded PXE runtime                                   |
| `aztec_rs::tx`         | `aztec_core::tx`                                   | `Tx`, `TxReceipt`, `TxStatus`, `ExecutionPayload`      |
| `aztec_rs::types`      | `aztec_core::types`                                | `Fr`, `Fq`, `AztecAddress`, `EthAddress`, `PublicKeys` |
| `aztec_rs::wallet`     | `aztec_wallet::*`                                  | `Wallet` trait, `BaseWallet`, `AccountProvider`        |

## Top-Level Items

- `aztec_rs::Error` — re-export of [`aztec_core::error::Error`](./errors.md), the canonical top-level error type used across the workspace.

## Quick Start

```rust,no_run
use aztec_rs::node::{create_aztec_node_client, wait_for_node, AztecNode};

# async fn example() -> Result<(), aztec_rs::Error> {
let node = create_aztec_node_client("http://localhost:8080");
let info = wait_for_node(&node).await?;
println!("Connected to node v{}", info.node_version);

let block = node.get_block_number().await?;
println!("Current block: {block}");
# Ok(())
# }
```

## Full API

The complete rustdoc is published alongside this book at [`api/aztec_rs/`](../api/aztec_rs/index.html).
Run `cargo doc --open` locally to regenerate it.

## See Also

- [Crate Index](./crates.md) — per-crate reference pages.
- [Errors](./errors.md) — full error taxonomy.
