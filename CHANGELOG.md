# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- None yet; everything in progress is targeted for the next release.

## [0.2.2] - 2026-04-07

### Added

- `FeePaymentMethod` async trait defining the fee payment strategy interface (`aztec-fee`)
- `NativeFeePaymentMethod` — pay fees using existing Fee Juice balance (`aztec-fee`)
- `SponsoredFeePaymentMethod` — gasless transactions via a sponsor contract (`aztec-fee`)
- `FeeJuicePaymentMethodWithClaim` — claim bridged Fee Juice from L1 and pay fees in one transaction (`aztec-fee`)
- `L2AmountClaim` type for L1-to-L2 bridge deposit claim data (`aztec-fee`)
- `FunctionSelector::from_signature()` — compute 4-byte selectors from Noir function signature strings via Keccak-256 (`aztec-core`)
- `ExecutionPayload::merge()` — combine multiple execution payloads with fee payer conflict detection (`aztec-core`)
- `protocol_contract_address::fee_juice()` — well-known Fee Juice contract address constant (`aztec-core`)
- Fee types and constants re-exported from the `aztec-rs` umbrella crate
- 30+ new unit tests across `aztec-core` (selectors, merge) and `aztec-fee` (all three payment methods)

## [0.2.1] - 2026-04-07

### Added

- `BaseWallet<P, N, A>` — production `Wallet` implementation backed by PXE + Aztec node connections (`aztec-wallet`)
- `AccountProvider` trait decoupling wallet implementations from specific account types (`aztec-wallet`)
- `SingleAccountProvider` for the common single-account wallet pattern (`aztec-account`)
- `create_wallet()` convenience factory function
- `send_tx`, `get_contract`, `get_contract_class` methods on `AztecNode` trait and `HttpNodeClient` (`aztec-node-client`)
- Private event decoding from PXE `PackedPrivateEvent` to wallet-level `PrivateEvent` objects
- PXE module re-exported from `aztec-wallet` crate
- 19 new unit tests for `BaseWallet` covering all `Wallet` trait methods with mock PXE/node/account backends
- `BaseWallet`, `AccountProvider`, `SingleAccountProvider`, and `create_wallet` re-exported from the `aztec-rs` umbrella crate
- PXE integration tests (`tests/pxe_integration.rs`) — 9 tests covering connectivity, account/sender lifecycle, contract queries, and wire-format roundtrips against a live PXE
- `BaseWallet` integration tests (`tests/wallet_integration.rs`) — 7 tests covering chain info, address book, contract metadata, and contract registration against a live PXE + node

### Fixed

- Aligned `aztec-pxe-client` request option payloads with upstream PXE semantics by adding `simulatePublic`, `overrides`, `profileMode`, `skipProofGeneration`, and `authwits` to the Rust wire types
- Corrected private event transport types in `aztec-pxe-client` to use upstream field names and metadata (`packedEvent`, `contractAddress`, `txHash`, `afterLog`, `l2BlockNumber`, `l2BlockHash`, `eventSelector`)
- Corrected `UtilityExecutionResult` to deserialize the upstream PXE response shape (`result` plus optional `stats`)
- Expanded PXE client tests to cover the corrected wire formats and added local `BlockHash` / `LogId` transport helpers needed for event parity
- Added `PartialEq` derive to `ExecuteUtilityOpts` for test assertions

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

[Unreleased]: https://github.com/NethermindEth/aztec-rust/compare/v0.2.2...HEAD
[0.2.2]: https://github.com/NethermindEth/aztec-rust/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/NethermindEth/aztec-rust/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/NethermindEth/aztec-rust/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/NethermindEth/aztec-rust/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/NethermindEth/aztec-rust/releases/tag/v0.1.0
