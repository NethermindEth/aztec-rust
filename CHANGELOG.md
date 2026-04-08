# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `Salt` type alias (`pub type Salt = Fr`) for deployment salt ergonomics (`aztec-core`)
- `AztecAddress::zero()` convenience constructor (`aztec-core`)
- `FunctionCall::empty()`, `FunctionCall::is_empty()` — canonical empty call for entrypoint payload padding (`aztec-core`)
- `FunctionCall.hide_msg_sender` field for controlling msg_sender visibility to callees (`aztec-core`)
- `FunctionSelector::empty()` — zero selector constant (`aztec-core`)
- `HashedValues::from_args()`, `HashedValues::from_calldata()`, `HashedValues::hash()` — helpers for entrypoint call encoding (`aztec-core`)
- `domain_separator::SIGNATURE_PAYLOAD` constant for entrypoint payload hashing (`aztec-core`)
- `get_contract_instance_from_instantiation_params()` — shared helper for computing contract instances from artifact + params, used by both generic deployment and account address pre-computation (`aztec-contract`)
- `ContractInstantiationParams` struct for the shared instance-construction helper (`aztec-contract`)
- `EncodedAppEntrypointCalls` — encodes function calls for account/multi-call entrypoint payloads with Poseidon2 hashing (`aztec-account`)
- `DefaultAccountEntrypoint` — standard account entrypoint wrapping calls through the account contract with auth witness creation (`aztec-account`)
- `AccountFeePaymentMethodOptions` enum (External, PreexistingFeeJuice, FeeJuiceWithClaim) (`aztec-account`)
- `DefaultAccountEntrypointOptions` for configuring cancellable, tx_nonce, and fee payment method (`aztec-account`)
- `DefaultMultiCallEntrypoint` — multi-call entrypoint for unsigned transactions via protocol contract address 4 (`aztec-account`)
- `SignerlessAccount` — account requiring no signing, routes through `DefaultMultiCallEntrypoint` for fee-sponsored and protocol-level operations (`aztec-account`)
- `AccountEntrypointMetaPaymentMethod` — wraps fee payment through account entrypoint for self-paying account deployments with auto-detection of fee payment options (`aztec-account`)
- `get_account_contract_address()` — computes deterministic account address before deployment using shared instance-construction helper (`aztec-account`)
- `DeployAccountOptions` struct with skip flags, fee payment, and fee entrypoint options (`aztec-account`)
- `DeployResult` struct (account-specific) returning `SendResult` + `ContractInstanceWithAddress` from account deployment (`aztec-account`)
- New modules: `entrypoint/`, `signerless`, `meta_payment` in `aztec-account`
- All new types re-exported from `aztec-account` and `aztec-rs` umbrella crate
- 30+ new unit tests across entrypoint encoding, account entrypoint, multi-call entrypoint, signerless account, and meta payment method

### Changed

- `AccountManager::create()` salt parameter now accepts `Option<impl Into<Salt>>` for ergonomic salt input (`aztec-account`)
- `AccountManager::create()` now computes real contract instance with derived keys, class ID, initialization hash, and deterministic address instead of using zero placeholders (`aztec-account`)
- `AccountManager::address()` now returns a real derived address (`aztec-account`)
- `DeployAccountMethod` replaced from a stub returning errors to a full implementation wrapping `aztec-contract::DeployMethod` with account-specific fee-payment logic (`aztec-account`)
- `DeployAccountMethod::request()` is now `async` and accepts `&DeployAccountOptions`; builds real deployment payloads with correct fee ordering (deploy-first for self-deploy, fee-first for third-party) (`aztec-account`)
- `DeployAccountMethod::simulate()` and `send()` signatures updated to accept `&DeployAccountOptions` (`aztec-account`)
- `DeployMethod::get_instance()` refactored to delegate to shared `get_contract_instance_from_instantiation_params()` helper (`aztec-contract`)
- `aztec-account` now depends on `aztec-contract` and `aztec-fee` for deployment reuse and fee payment integration
- Updated `examples/account_flow.rs` to demonstrate full lifecycle: real address derivation, deployment payload construction, and `get_account_contract_address()` verification

## [0.3.0] - 2026-04-08

### Added

- Full key derivation pipeline in `aztec-crypto`: `derive_keys`, `derive_master_nullifier_hiding_key`, `derive_master_incoming_viewing_secret_key`, `derive_master_outgoing_viewing_secret_key`, `derive_master_tagging_secret_key`, `derive_signing_key`, `derive_public_key_from_secret_key` (`aztec-crypto`)
- `sha512_to_grumpkin_scalar` — SHA-512 hash reduced to a Grumpkin scalar for master key derivation (`aztec-crypto`)
- `DerivedKeys` struct containing all four master secret keys and their corresponding `PublicKeys` (`aztec-crypto`)
- App-scoped key derivation: `KeyType` enum, `compute_app_secret_key`, `compute_app_nullifier_hiding_key`, `compute_ovsk_app` (`aztec-crypto`)
- `complete_address_from_secret_key_and_partial_address` for deriving a `CompleteAddress` from a secret key and partial address (`aztec-crypto`)
- `compute_address` — extracted address derivation from public keys and partial address as a standalone function (`aztec-core`)
- `compute_secret_hash` — Poseidon2 hash with `SECRET_HASH` domain separator for L1-L2 messages (`aztec-core`)
- Domain separators for key derivation: `NHK_M`, `IVSK_M`, `OVSK_M`, `TSK_M`, `SECRET_HASH` (`aztec-core`)
- `Fq` type expanded with `zero()`, `one()`, `to_be_bytes()`, `from_be_bytes_mod_order()`, `is_zero()`, `random()`, `hi()`, `lo()`, `From<u64>`, `From<[u8; 32]>`, `Mul` (`aztec-core`)
- `aztec-rs::crypto` module re-exporting the full `aztec-crypto` public API from the umbrella crate
- `sha2` dependency added to `aztec-crypto`
- 15+ new unit tests cross-validated against TypeScript SDK test vectors (secret_key = 8923)

### Changed

- `GrumpkinScalar` type alias changed from `Fr` to `Fq` for type correctness — Grumpkin scalars live in the BN254 base field (`aztec-core`)
- `grumpkin::scalar_mul` now accepts `&Fq` instead of `&Fr` as the scalar argument (`aztec-core`)
- `compute_contract_address_from_instance` refactored to delegate to the new `compute_address` function (`aztec-core`)
- `AccountManager::complete_address()` now performs real key derivation and address computation instead of returning an error (`aztec-account`)
- `aztec-account` now depends on `aztec-crypto` for key derivation

## [0.2.5] - 2026-04-08

### Added

- `Fr::to_be_bytes()`, `Fr::to_usize()`, `Fr::is_zero()` public helper methods on the scalar field type (`aztec-core`)
- `From<i64>`, `From<u128>`, `From<bool>`, `From<AztecAddress>`, `From<EthAddress>`, `From<FunctionSelector>` conversions for `Fr` (`aztec-core`)
- `From<u64>` conversion for `AztecAddress` (`aztec-core`)
- `FieldLike`, `AztecAddressLike`, `EthAddressLike` type aliases mirroring TypeScript SDK union types (`aztec-core`)
- ABI type checker functions: `is_address_struct`, `is_aztec_address_struct`, `is_eth_address_struct`, `is_function_selector_struct`, `is_wrapped_field_struct`, `is_public_keys_struct`, `is_bounded_vec_struct`, `is_option_struct` (`aztec-core`)
- `abi_type_size()` and `count_arguments_size()` for computing flattened field-element counts from ABI types (`aztec-core`)
- Enhanced `encode_arguments()` with special-case handling for Address, FunctionSelector, wrapped-field, BoundedVec, and Option structs (`aztec-core`)
- Signed integer encoding using proper two's complement (safe for widths > 64 bits), replacing the previous `as u64` truncation (`aztec-core`)
- `AbiDecoded` enum and `decode_from_abi()` for reconstructing typed values from flat field-element arrays (`aztec-core`)
- `NoteSelector` type — 7-bit note selector with validation, field/hex/serde roundtrips (`aztec-core`)
- `ContractArtifact::to_buffer()`, `from_buffer()`, `to_json()` serialization methods (`aztec-core`)
- `buffer_as_fields()` / `buffer_from_fields()` for round-tripping byte buffers through field elements (`aztec-core`)
- 60+ new unit tests covering all new functionality including encode-decode roundtrips

### Changed

- Split `abi.rs` into sub-modules (`abi/types.rs`, `abi/selectors.rs`, `abi/checkers.rs`, `abi/encoder.rs`, `abi/decoder.rs`, `abi/buffer.rs`) with `abi/mod.rs` re-exporting the full public API (`aztec-core`)
- `hash.rs` private helpers `fr_to_be_bytes` / `fr_to_usize` now delegate to the new public `Fr` methods (`aztec-core`)

## [0.2.4] - 2026-04-08

### Added

- Grumpkin curve module with affine point addition, doubling, and scalar multiplication for contract address derivation (`aztec-core`)
- Contract class ID computation: `compute_private_functions_root`, `compute_artifact_hash`, `compute_public_bytecode_commitment`, `compute_contract_class_id`, `compute_contract_class_id_from_artifact` (`aztec-core`)
- Contract address derivation: `compute_salted_initialization_hash`, `compute_partial_address`, `compute_contract_address_from_instance` using Grumpkin EC operations (`aztec-core`)
- Initialization hash computation: `compute_initialization_hash`, `compute_initialization_hash_from_encoded` (`aztec-core`)
- `PublicKeys::hash()` with `PUBLIC_KEYS_HASH` domain separator and empty-key shortcut (`aztec-core`)
- `PublicKeys::is_empty()` and `Point::is_zero()` helpers (`aztec-core`)
- Domain separators: `PUBLIC_KEYS_HASH`, `PARTIAL_ADDRESS`, `CONTRACT_CLASS_ID`, `PRIVATE_FUNCTION_LEAF`, `PUBLIC_BYTECODE`, `INITIALIZER`, `CONTRACT_ADDRESS_V1` (`aztec-core`)
- Protocol contract addresses: `contract_instance_deployer` (2), `contract_class_registerer` (3), `multi_call_entrypoint` (4) (`aztec-core`)
- Size constants: `FUNCTION_TREE_HEIGHT`, `MAX_PACKED_PUBLIC_BYTECODE_SIZE_IN_FIELDS`, `ARTIFACT_FUNCTION_TREE_MAX_HEIGHT` (`aztec-core`)
- `FunctionArtifact` fields: `bytecode`, `verification_key_hash`, `verification_key`, `custom_attributes`, `is_unconstrained`, `debug_symbols` — all `Option` with `#[serde(default)]` (`aztec-core`)
- `ContractArtifact` fields: `outputs`, `file_map` — both `Option` with `#[serde(default)]` (`aztec-core`)
- `FunctionSelector::from_name_and_parameters()` for deriving selectors from ABI metadata (`aztec-core`)
- `abi_type_signature()` helper converting `AbiType` to canonical Noir signature strings (`aztec-core`)
- `buffer_as_fields()` utility for encoding byte buffers as field elements (31 bytes per field) (`aztec-core`)
- `publish_contract_class()` — builds interaction payload targeting the Contract Class Registerer with bytecode capsule (`aztec-contract`)
- `publish_instance()` — builds interaction payload targeting the Contract Instance Deployer with universal deploy flag (`aztec-contract`)
- `DeployMethod::get_instance()` now computes real contract class ID, initialization hash, and deterministic address (`aztec-contract`)
- `DeployMethod::request()` fully wires registration, class publication, instance publication, and constructor call (`aztec-contract`)
- `DeployResult` struct returning `SendResult` + `ContractInstanceWithAddress` from `DeployMethod::send()` (`aztec-contract`)
- `SuggestedGasLimits` struct and `get_gas_limits()` with configurable padding factor (`aztec-contract`)
- `DeployOptions::from` field for explicit deployer address selection (`aztec-contract`)
- `ContractFunctionInteraction::new()` and `new_with_capsules()` constructors (`aztec-contract`)
- `sha2` crate dependency for artifact hash computation (`aztec-core`)
- 30+ new unit tests across `aztec-core` (grumpkin, deployment hashes, address derivation) and `aztec-contract` (gas limits, class/instance publication, full deployment flow)

### Changed

- `Capsule` struct now has `contract_address: AztecAddress`, `storage_slot: Fr`, and `data: Vec<Fr>` fields instead of `data: Vec<u8>` (`aztec-core`)
- `DeployMethod::request()` is now `async` and returns real deployment payloads instead of stub errors (`aztec-contract`)
- `DeployMethod::get_instance()` now returns `Result<ContractInstanceWithAddress, Error>` (was infallible) (`aztec-contract`)
- `DeployMethod::send()` now returns `DeployResult` containing both the tx hash and deployed instance (`aztec-contract`)
- `ContractFunctionInteraction` now carries an optional `capsules` field included in generated payloads (`aztec-contract`)

## [0.2.3] - 2026-04-08

### Added

- Poseidon2 hash module with `poseidon2_hash_with_separator`, `poseidon2_hash_bytes`, `compute_var_args_hash` matching barretenberg/TS SDK output (`aztec-core`)
- Authwit hash computation functions: `compute_inner_auth_wit_hash`, `compute_outer_auth_wit_hash`, `compute_inner_auth_wit_hash_from_action`, `compute_auth_wit_message_hash` (`aztec-core`)
- `abi_values_to_fields` helper for flattening `AbiValue` to `Vec<Fr>` for hash input (`aztec-core`)
- Domain separator constants: `AUTHWIT_INNER`, `AUTHWIT_OUTER`, `FUNCTION_ARGS` (`aztec-core`)
- `protocol_contract_address::auth_registry()` — AuthRegistry protocol contract at address 1 (`aztec-core`)
- `AuthWitness.request_hash` field identifying which message a witness authorizes (`aztec-core`)
- `MessageHashOrIntent::InnerHash` variant for pre-computed inner hashes with consumer address (`aztec-core`)
- `CallAuthorizationRequest` struct with `new()`, `selector()`, and `from_fields()` for authwit preimage handling (`aztec-account`)
- `AuthorizationSelector` type for authorization request type identification (`aztec-core`)
- `FunctionSelector::from_field()` and `FunctionSelector::to_field()` field-element conversions (`aztec-core`)
- `SetPublicAuthWitInteraction` — convenience type for setting public authwits in the AuthRegistry (`aztec-contract`)
- `lookup_validity()` — check authwit validity in both private and public contexts (`aztec-contract`)
- `AuthWitValidity` result type for validity checks (`aztec-contract`)
- Intent-to-hash resolution in `SingleAccountProvider.create_auth_wit()` so `AuthorizationProvider` implementations always receive resolved hashes (`aztec-account`)
- Hash functions, authwit helpers, and authorization types re-exported from the `aztec-rs` umbrella crate
- 25+ new unit tests across `aztec-core` (hash, constants), `aztec-account` (authorization), and `aztec-contract` (authwit)

### Changed

- `ChainInfo` and `MessageHashOrIntent` moved from `aztec-wallet` to `aztec-core::hash` to avoid circular dependencies; re-exported from `aztec-wallet` for backward compatibility

### Fixed

- Corrected selector derivation to use Aztec-compatible Poseidon2-over-bytes semantics rather than Keccak-based hashing (`aztec-core`)
- Added `AuthorizationSelector`-aware authwit request decoding so `CallAuthorizationRequest` now validates the upstream selector field and deserializes the correct field order (`aztec-account`)
- Corrected authwit validity helper selectors for Aztec address-typed arguments (`lookup_validity((Field),Field)` and `utility_is_consumable((Field),Field)`) and added `SetPublicAuthWitInteraction::profile()` parity (`aztec-contract`)
- Removed the now-unused `sha3` dependency from `aztec-core`

## [0.2.2] - 2026-04-07

### Added

- `FeePaymentMethod` async trait defining the fee payment strategy interface (`aztec-fee`)
- `NativeFeePaymentMethod` — pay fees using existing Fee Juice balance (`aztec-fee`)
- `SponsoredFeePaymentMethod` — gasless transactions via a sponsor contract (`aztec-fee`)
- `FeeJuicePaymentMethodWithClaim` — claim bridged Fee Juice from L1 and pay fees in one transaction (`aztec-fee`)
- `L2AmountClaim` type for L1-to-L2 bridge deposit claim data (`aztec-fee`)
- `FunctionSelector::from_signature()` — compute 4-byte selectors from Noir function signature strings (`aztec-core`)
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

[Unreleased]: https://github.com/NethermindEth/aztec-rust/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/NethermindEth/aztec-rust/compare/v0.2.5...v0.3.0
[0.2.5]: https://github.com/NethermindEth/aztec-rust/compare/v0.2.4...v0.2.5
[0.2.4]: https://github.com/NethermindEth/aztec-rust/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/NethermindEth/aztec-rust/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/NethermindEth/aztec-rust/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/NethermindEth/aztec-rust/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/NethermindEth/aztec-rust/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/NethermindEth/aztec-rust/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/NethermindEth/aztec-rust/releases/tag/v0.1.0
