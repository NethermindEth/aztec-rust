# Concepts Overview

This section introduces the mental model behind `aztec-rs`.
Read it before the [Architecture](../architecture/overview.md) chapters if you are new to Aztec.

## Context

Aztec is a privacy-first L2 with a client-side prover model.
Private state lives in **notes**, held by users, and is consumed inside a **PXE** (Private Execution Environment) that runs locally.
Public state lives on the rollup and is accessed via the node.

`aztec-rs` packages the client-side runtime — PXE, wallet, accounts, contract tooling — as a Rust workspace.

## Design

`aztec-rs` is organized around three boundaries:

1. **Client runtime** — the PXE runs in-process and owns private state.
2. **Wallet/account layer** — builds authenticated transactions on top of PXE and an account provider.
3. **Node / L1 clients** — thin transport crates that speak JSON-RPC to an Aztec node and HTTP/RPC to Ethereum.

## Key Topics

- [Terminology](./terminology.md) — shared vocabulary.
- [PXE](./pxe.md) — how private execution happens locally.
- [Accounts & Wallets](./accounts-and-wallets.md) — account abstraction flavors and entrypoints.
- [Contracts](./contracts.md) — artifacts, private/public/utility functions.
- [Fees](./fees.md) — payment methods and fee juice.
- [Cross-Chain Messaging](./cross-chain-messaging.md) — L1 ↔ L2 messaging.

## References

- [Aztec protocol docs](https://docs.aztec.network)
- [Aztec Packages monorepo](https://github.com/AztecProtocol/aztec-packages)
