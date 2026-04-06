# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Restructured codebase from a single flat crate into a Cargo workspace with 10 internal crates (`aztec-core`, `aztec-crypto`, `aztec-rpc`, `aztec-node-client`, `aztec-pxe-client`, `aztec-wallet`, `aztec-contract`, `aztec-account`, `aztec-fee`, `aztec-ethereum`)
- Migrated all existing modules into their respective workspace crates while preserving the public API via umbrella re-exports in `aztec-rs`
- Root `Cargo.toml` now defines a workspace and the `aztec-rs` umbrella crate depends on all workspace members

### Added

- Stub crates for future functionality: `aztec-crypto`, `aztec-pxe-client`, `aztec-fee`, `aztec-ethereum`
- Gap analysis and refactor plan documents (`GAP_ANALYSIS.md`, `REFACTOR_PLAN.md`)

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

[Unreleased]: https://github.com/NethermindEth/aztec-rust/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/NethermindEth/aztec-rust/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/NethermindEth/aztec-rust/releases/tag/v0.1.0
