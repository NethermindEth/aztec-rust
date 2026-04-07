# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Aligned `aztec-pxe-client` request option payloads with upstream PXE semantics by adding `simulatePublic`, `overrides`, `profileMode`, `skipProofGeneration`, and `authwits` to the Rust wire types
- Corrected private event transport types in `aztec-pxe-client` to use upstream field names and metadata (`packedEvent`, `contractAddress`, `txHash`, `afterLog`, `l2BlockNumber`, `l2BlockHash`, `eventSelector`)
- Corrected `UtilityExecutionResult` to deserialize the upstream PXE response shape (`result` plus optional `stats`)
- Expanded PXE client tests to cover the corrected wire formats and added local `BlockHash` / `LogId` transport helpers needed for event parity

## [0.2.0] - 2026-04-07

### Added

- `Pxe` trait in `aztec-pxe-client` mirroring the TypeScript PXE interface (18 methods: `simulate_tx`, `prove_tx`, `register_account`, `register_contract`, `get_private_events`, etc.)
- `HttpPxeClient` — HTTP/JSON-RPC client for connecting to a running PXE node
- `create_pxe_client(url)` factory function with 30s default timeout
- `wait_for_pxe()` polling helper (120s timeout, 1s interval)
- PXE-specific types: `BlockHeader`, `TxExecutionRequest`, `TxProvingResult`, `TxSimulationResult`, `TxProfileResult`, `UtilityExecutionResult`, `PackedPrivateEvent`, `PrivateEventFilter`, `RegisterContractRequest`
- PXE option types: `SimulateTxOpts`, `ProfileTxOpts`, `ExecuteUtilityOpts`
- `RpcTransport::call_void()` for void-returning JSON-RPC methods
- PXE module re-exported from the `aztec-rs` umbrella crate
- 34 unit tests covering serde roundtrips, mock PXE, trait safety, and polling

### Changed

- Restructured codebase from a single flat crate into a Cargo workspace with 10 internal crates (`aztec-core`, `aztec-crypto`, `aztec-rpc`, `aztec-node-client`, `aztec-pxe-client`, `aztec-wallet`, `aztec-contract`, `aztec-account`, `aztec-fee`, `aztec-ethereum`)
- Migrated all existing modules into their respective workspace crates while preserving the public API via umbrella re-exports in `aztec-rs`
- Root `Cargo.toml` now defines a workspace and the `aztec-rs` umbrella crate depends on all workspace members

### Removed

- Flat `src/*.rs` module files (code moved into workspace crates)

## [0.1.1] - 2026-04-06

### Fixed

- README installation instructions

## [0.1.0] - 2026-04-06

### Added

- Core Aztec SDK types (addresses, hashes, keys, fields, transactions, logs)
- Aztec node JSON-RPC client
- Wallet API aligned with Aztec.js semantics
- Contract interaction and deployment modules
- Event decoding and filter support
- Account model with entrypoint abstraction
- `BatchCall` contract interaction helper
- Contract artifact fixtures for testing
- Integration tests and deployment/account examples
- Documentation comments on all public types
- Project README and fixture artifacts README
- CI/CD workflows for GitHub Actions

### Changed

- License from MIT/Apache-2.0 dual to Apache-2.0 only

### Removed

- Implementation plan and spec documents

[Unreleased]: https://github.com/NethermindEth/aztec-rust/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/NethermindEth/aztec-rust/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/NethermindEth/aztec-rust/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/NethermindEth/aztec-rust/releases/tag/v0.1.0
