# Introduction

`aztec-rs` is a Rust workspace that provides a full client-side runtime for the [Aztec Network](https://aztec.network).
It bundles an embedded PXE (Private Execution Environment), wallet and account abstractions, contract tooling, a node client, and L1 / Ethereum helpers.

This book is the entry point for users, application developers, and contributors.

The docs are organized around what you are trying to do, then link down into API details.
If a page starts to feel like a crate inventory, jump to its **Start From User Tasks** section or the matching runnable example.

## Who This Is For

| Audience    | Start here                                                              |
| ----------- | ----------------------------------------------------------------------- |
| Users       | [Quickstart](./quickstart.md), [Guides](./guides/installation.md), and the runnable examples listed there |
| Developers  | [Choose by task](./reference/crates.md) and then drill into per-crate reference |
| Engineers   | [Architecture](./architecture/overview.md) and [Concepts](./concepts/overview.md) |
| Contributors| [Development](./development/contributing.md)                             |

## What's Inside

- **Embedded PXE runtime** — in-process private execution engine with note discovery, local stores, kernel simulation/proving, and block sync.
- **PXE client surface** — `aztec-pxe-client` defines the PXE trait and shared request/response types used by wallets and runtimes.
- **Wallet runtime** — `BaseWallet` composes a PXE backend, Aztec node client, and account provider into a production wallet.
- **Node client** — connect to an Aztec node over JSON-RPC, query blocks, chain info, and wait for readiness.
- **Contract interaction** — load contract artifacts, build and send function calls (private, public, utility).
- **Contract deployment** — builder-pattern deployer with deterministic addressing, class registration, and instance publication.
- **Account abstraction** — Schnorr and signerless account flavors, entrypoint execution, and authorization witnesses.
- **Fee payments** — native, sponsored, and Fee Juice claim-based payment methods.
- **Cross-chain messaging** — L1-to-L2 and L2-to-L1 message sending, readiness polling, and consumption.
- **Cryptography** — BN254/Grumpkin field arithmetic, Poseidon2, Pedersen, Schnorr signing, key derivation.

## Conventions Used in This Book

- Code samples are Rust unless otherwise marked.
- Shell commands assume `bash`/`zsh` on macOS or Linux.
- Paths are relative to the repository root.
- Normative keywords (MUST, SHOULD, MAY) appear only in protocol and specification sections.

## Project Status

Active development.
APIs may still change.
See the [README](https://github.com/NethermindEth/aztec-rs/blob/main/README.md) and [CHANGELOG](https://github.com/NethermindEth/aztec-rs/blob/main/CHANGELOG.md) for the latest release notes.

## License

Licensed under the [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0).
