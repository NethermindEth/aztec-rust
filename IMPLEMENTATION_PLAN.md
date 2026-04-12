# aztec-rs Implementation Plan

**Reviewed against code on:** 2026-04-12  
**Rust workspace version:** `0.3.2`

This document is based on direct code comparison between:
- Rust workspace: `crates/*` in this repository
- Upstream TypeScript workspace: `/Users/alexmetelli/source/aztec-packages/yarn-project`

## Method

- An item is marked `missing` only when upstream exports a concrete file/symbol and no corresponding Rust symbol was found.
- An item is marked `partial` only when Rust has a corresponding module or API surface but the implementation is narrower, contains explicit `TODO` placeholders, or contains explicit `unimplemented` / `not implemented yet` paths.
- This plan intentionally avoids parity percentages. It tracks file-level deltas.

---

## Snapshot

| Area | Upstream basis | Rust status | Summary |
|------|----------------|-------------|---------|
| PXE runtime | `yarn-project/pxe/src/pxe.ts`, `yarn-project/pxe/src/private_kernel/*` | Partial | Core embedded runtime exists in `crates/pxe`, but several upstream orchestration and oracle paths are still missing or explicitly incomplete. |
| Wallet API | `yarn-project/aztec.js/src/wallet/wallet.ts`, `capabilities.ts` | Partial | Core wallet exists, but capability APIs, batch-at-wallet level, and wait/result ergonomics are narrower than upstream. |
| Contract interaction API | `yarn-project/aztec.js/src/contract/*` | Partial | Contract deployment and interactions exist, but the higher-level interaction option model is still slimmer than upstream. |
| Fee methods | `yarn-project/aztec.js/src/fee/*` | Partial | Native/sponsored/claim-based fee methods exist; private/public fee methods do not. |
| Account/auth | `yarn-project/aztec.js/src/account/*`, `wallet/account_manager.ts` | Mostly implemented | Account manager, signerless account, deploy method, meta-payment, and authwit hashing/requests are present. |
| Messaging + Ethereum | `yarn-project/aztec.js/src/api/messaging.ts`, `utils/cross_chain.ts`, `ethereum/portal_manager.ts` | Missing | `crates/ethereum` exists but contains no concrete implementation beyond an empty `messaging` module. |
| Public API types | `yarn-project/aztec.js/src/api/*.ts` | Partial | Several upstream API exports already exist in Rust; several protocol/block/tree/log/note wrappers still do not. |
| Utilities | `yarn-project/aztec.js/src/utils/*` | Partial | Some utility parity exists in `aztec-core` / `aztec-crypto`, but `field_compressed_string` and `fee_juice` helpers are missing. |

---

## 1. PXE Runtime Parity

### Compared files

- Upstream:
  - `yarn-project/pxe/src/pxe.ts`
  - `yarn-project/pxe/src/private_kernel/private_kernel_oracle.ts`
  - `yarn-project/pxe/src/private_kernel/private_kernel_execution_prover.ts`
  - `yarn-project/pxe/src/private_kernel/hints/compute_tx_expiration_timestamp.ts`
  - `yarn-project/pxe/src/debug/pxe_debug_utils.ts`
- Rust:
  - `crates/pxe/src/embedded_pxe.rs`
  - `crates/pxe/src/kernel/oracle.rs`
  - `crates/pxe/src/kernel/execution_prover.rs`
  - `crates/pxe/src/execution/oracle.rs`
  - `crates/pxe/src/execution/utility_oracle.rs`
  - `crates/pxe/src/sync/*`
  - `crates/pxe/src/stores/*`
  - `crates/pxe-client/src/pxe.rs`

### Already present in Rust

- In-process PXE runtime in `crates/pxe` via `EmbeddedPxe`
- Shared `Pxe` trait and PXE request/result/filter types in `crates/pxe-client`
- Local stores for contracts, keys, addresses, notes, capsules, senders, recipient tagging, and private events
- Block sync and contract sync services
- Private execution / utility execution / proving scaffolding
- Protocol-contract-specific handling paths in `crates/pxe/src/embedded_pxe.rs`

### Concrete code deltas

- Upstream `PXE.create(...)` in `yarn-project/pxe/src/pxe.ts` wires `JobCoordinator`, `SerialQueue`, `PXEDebugUtils`, and `#registerProtocolContracts()`. No matching `JobCoordinator`, `SerialQueue`, `PXEDebugUtils`, or protocol-artifact registration entrypoint exists in `crates/pxe`.
- Rust still contains explicit unimplemented oracle gaps:
  - `crates/pxe/src/kernel/oracle.rs`: private function membership witness
  - `crates/pxe/src/kernel/oracle.rs`: protocol VK membership witness
  - `crates/pxe/src/kernel/oracle.rs`: updated class-id hints
- Rust still contains explicit PXE placeholders:
  - `crates/pxe/src/embedded_pxe.rs`: `TxConstantData::default()` placeholder
  - `crates/pxe/src/embedded_pxe.rs`: expiration timestamp hardcoded to `0`
- Upstream has a dedicated expiration-timestamp helper in `yarn-project/pxe/src/private_kernel/hints/compute_tx_expiration_timestamp.ts`; no corresponding Rust helper was found.
- Rust `crates/pxe/src/execution/oracle.rs` still returns `"aes128Decrypt not implemented"` and `"getSharedSecret not implemented"` for specific foreign calls.

### Planned work

1. Close the explicit oracle gaps in `crates/pxe/src/kernel/oracle.rs`.
2. Replace the block-header and expiration placeholders in `crates/pxe/src/embedded_pxe.rs`.
3. Decide whether protocol contract registration should mirror upstream as an explicit initialization step or remain embedded in special-case runtime handling.
4. Add focused tests around the currently incomplete oracle and proving paths rather than broad PXE smoke tests only.

---

## 2. Wallet API Parity

### Compared files

- Upstream:
  - `yarn-project/aztec.js/src/wallet/wallet.ts`
  - `yarn-project/aztec.js/src/wallet/capabilities.ts`
- Rust:
  - `crates/wallet/src/wallet.rs`
  - `crates/wallet/src/base_wallet.rs`
  - `crates/wallet/src/account_provider.rs`

### Already present in Rust

- `Wallet` trait
- `BaseWallet`
- `create_wallet(...)`
- `simulate_tx`, `execute_utility`, `profile_tx`, `send_tx`
- `create_auth_wit`
- contract registration and metadata lookups
- private event retrieval
- `wait_for_tx_proven(...)` on wallet side

### Concrete code deltas

- Upstream `Wallet` includes `requestCapabilities(...)`; no corresponding Rust wallet trait method exists.
- Upstream `Wallet` includes `batch(...)`; no corresponding Rust wallet trait method exists.
- Upstream `SendOptions` and `sendTx` support `NO_WAIT` and typed wait-sensitive returns; Rust `send_tx` returns only `SendResult { tx_hash }`.
- Upstream wallet schemas include capability-related types and schemas from `capabilities.ts`; no corresponding Rust capability types were found.
- Upstream wallet types include `PublicEventFilter` / `PublicEvent` in `wallet.ts`; Rust public event support exists, but it lives in `crates/contract/src/events.rs`, not in the wallet trait surface.

### Planned work

1. Add wallet capability request/grant types only if the Rust API wants to mirror upstream wallet-scoped permissions.
2. Decide whether `batch(...)` belongs on `Wallet` or whether `BatchCall` in `aztec-contract` is the final Rust shape.
3. Add an explicit no-wait mode if parity with upstream interaction ergonomics is required.

---

## 3. Contract Interaction Parity

### Compared files

- Upstream:
  - `yarn-project/aztec.js/src/contract/contract_function_interaction.ts`
  - `yarn-project/aztec.js/src/contract/interaction_options.ts`
  - `yarn-project/aztec.js/src/contract/wait_opts.ts`
  - `yarn-project/aztec.js/src/contract/contract_base.ts`
- Rust:
  - `crates/contract/src/contract.rs`
  - `crates/contract/src/deployment.rs`
  - `crates/node-client/src/node.rs`
  - `crates/wallet/src/wallet.rs`
  - `crates/core/src/abi/storage_layout.rs`

### Already present in Rust

- `Contract::deploy(...)`
- `Contract::deploy_with_public_keys(...)`
- `ContractFunctionInteraction`
- `BatchCall`
- `get_gas_limits(...)`
- richer `WaitOpts` with `wait_for_status`, `dont_throw_on_revert`, `ignore_dropped_receipts_for`
- `ContractStorageLayout` already exists in `crates/core/src/abi/storage_layout.rs`

### Concrete code deltas

- Upstream exports `NO_WAIT` in `interaction_options.ts`; no corresponding Rust constant was found.
- Upstream interaction option model includes `includeMetadata`, `paymentMethod`, `estimatedGasPadding`, `OffchainOutput`, `extractOffchainOutput`, and `estimatedGas`; no corresponding Rust types/helpers were found.
- Upstream `ContractFunctionInteraction.simulate()` decodes return values and can surface offchain messages/effects; Rust interaction methods forward raw wallet results and do not expose offchain output helpers.
- Upstream `ContractBase` builds a dynamic `methods` map and exports a `ContractMethod` type; Rust only exposes `Contract::method(name, args)`.
- Rust `crates/wallet/src/base_wallet.rs` still has `gas_used: None` with an explicit TODO for result extraction.

### Planned work

1. Add `NO_WAIT` only if `send_tx` return typing is expanded at the same time.
2. Decide whether Rust should adopt upstream-style `InteractionOptions` or keep the lower-level `fee_execution_payload` design.
3. Either add `ContractMethod` / dynamic method map parity or explicitly document `Contract::method(...)` as the intended Rust-only shape.
4. Finish gas/result extraction in `crates/wallet/src/base_wallet.rs`.

---

## 4. Fee Method Parity

### Compared files

- Upstream:
  - `yarn-project/aztec.js/src/fee/fee_payment_method.ts`
  - `yarn-project/aztec.js/src/fee/sponsored_fee_payment.ts`
  - `yarn-project/aztec.js/src/fee/fee_juice_payment_method_with_claim.ts`
  - `yarn-project/aztec.js/src/fee/private_fee_payment_method.ts`
  - `yarn-project/aztec.js/src/fee/public_fee_payment_method.ts`
- Rust:
  - `crates/fee/src/fee_payment_method.rs`
  - `crates/fee/src/sponsored.rs`
  - `crates/fee/src/fee_juice_with_claim.rs`
  - `crates/fee/src/native.rs`

### Already present in Rust

- `SponsoredFeePaymentMethod`
- `FeeJuicePaymentMethodWithClaim`
- `NativeFeePaymentMethod`
- account-deployment meta-payment wrapper in `crates/account/src/meta_payment.rs`

### Concrete code deltas

- Upstream exports `PrivateFeePaymentMethod`; no corresponding Rust symbol exists.
- Upstream exports `PublicFeePaymentMethod`; no corresponding Rust symbol exists.
- Upstream `FeePaymentMethod` interface exposes `getExecutionPayload()` and `getGasSettings()`; Rust `FeePaymentMethod` exposes `get_fee_execution_payload()` but no gas-settings accessor.
- Upstream interaction code merges `paymentMethod.getGasSettings()` into send/simulate/profile options; Rust has no equivalent fee-method-driven gas-settings path.

### Planned work

1. Add private/public fee methods only if the Rust interaction model also gains the gas-settings path upstream relies on.
2. Decide whether `getGasSettings()` parity belongs on the Rust fee trait or whether Rust should keep gas settings separate from fee methods.

---

## 5. Account and Auth Parity

### Compared files

- Upstream:
  - `yarn-project/aztec.js/src/account/account.ts`
  - `yarn-project/aztec.js/src/account/signerless_account.ts`
  - `yarn-project/aztec.js/src/wallet/account_manager.ts`
  - `yarn-project/aztec.js/src/wallet/deploy_account_method.ts`
  - `yarn-project/aztec.js/src/authorization/call_authorization_request.ts`
- Rust:
  - `crates/account/src/account.rs`
  - `crates/account/src/signerless.rs`
  - `crates/account/src/meta_payment.rs`
  - `crates/account/src/authorization.rs`
  - `crates/contract/src/authwit.rs`

### Already present in Rust

- `AccountManager`
- `DeployAccountMethod`
- `SignerlessAccount`
- `AccountEntrypointMetaPaymentMethod`
- `CallAuthorizationRequest`
- authwit hashing and validity helpers

### Concrete code deltas

- No major upstream account-manager feature gap was found beyond the broader wallet/interaction gaps already listed.
- Rust `SignerlessAccount` still panics on `complete_address()` and `address()` access. If upstream behavior is expected to be non-panicking here, this needs an explicit API decision.
- `crates/account/src/meta_payment.rs` still contains an `unimplemented!()` in test-only mock code.

### Planned work

1. Keep account/auth work scoped to wallet/interaction parity and integration tests unless a concrete upstream account symbol is later found missing.
2. Decide whether `SignerlessAccount` panics are acceptable API design or should become fallible accessors.

---

## 6. Messaging and Ethereum Parity

### Compared files

- Upstream:
  - `yarn-project/aztec.js/src/api/messaging.ts`
  - `yarn-project/aztec.js/src/utils/cross_chain.ts`
  - `yarn-project/aztec.js/src/ethereum/portal_manager.ts`
- Rust:
  - `crates/ethereum/src/lib.rs`
  - `crates/ethereum/src/messaging.rs`

### Already present in Rust

- Crate shell only

### Concrete code deltas

- Upstream exports `L1ToL2Message`, `L1Actor`, `L2Actor`; no corresponding Rust types were found.
- Upstream exports `isL1ToL2MessageReady(...)`; no corresponding Rust function was found.
- Upstream exports `waitForL1ToL2MessageReady(...)`; no corresponding Rust function was found.
- Upstream exports `generateClaimSecret(...)`; no corresponding Rust function was found.
- Upstream exports `L1TokenManager`, `L1FeeJuicePortalManager`, and `L1ToL2TokenPortalManager`; no corresponding Rust symbols were found.
- `crates/ethereum/src/messaging.rs` is currently empty.

### Planned work

1. Start by implementing the small messaging layer from `api/messaging.ts` and `utils/cross_chain.ts`.
2. Add the message / actor types before any portal-manager work.
3. Only after that, implement the portal managers from `portal_manager.ts`.

---

## 7. Public API Type Parity

### Compared files

- Upstream:
  - `yarn-project/aztec.js/src/api/protocol.ts`
  - `yarn-project/aztec.js/src/api/block.ts`
  - `yarn-project/aztec.js/src/api/trees.ts`
  - `yarn-project/aztec.js/src/api/log.ts`
  - `yarn-project/aztec.js/src/api/note.ts`
- Rust:
  - `crates/core/src/constants.rs`
  - `crates/core/src/kernel_types.rs`
  - `crates/core/src/abi/storage_layout.rs`
  - `crates/node-client/src/node.rs`
  - `crates/contract/src/events.rs`

### Already present in Rust

- protocol contract address constants in `crates/core/src/constants.rs`
- `GlobalVariables` in `crates/core/src/kernel_types.rs`
- public log querying via `PublicLogFilter` in `crates/node-client`
- public event decoding in `crates/contract/src/events.rs`
- `ContractStorageLayout` in `crates/core/src/abi/storage_layout.rs`

### Concrete code deltas

- Upstream re-exports `ProtocolContractAddress`; no corresponding Rust enum/class wrapper was found.
- Upstream re-exports protocol contract wrapper classes such as `AuthRegistryContract`, `FeeJuiceContract`, and registry wrappers; no corresponding Rust wrappers were found.
- Upstream re-exports `Body`, `L2Block`, and `getTimestampRangeForEpoch`; no corresponding Rust symbols were found.
- Upstream re-exports `SiblingPath` and `MerkleTreeId`; no corresponding Rust symbols were found.
- Upstream re-exports generic `LogFilter`; Rust has `PublicLogFilter` only.
- Upstream re-exports `Comparator` and `Note`; no corresponding public Rust symbols were found, although note-store internals and select-clause comparators exist inside `crates/pxe`.

### Planned work

1. Add the missing public type exports that are pure data-model parity first.
2. Add protocol contract wrappers only if they fit the Rust contract API instead of becoming thin aliases with no ergonomic value.

---

## 8. Utility Parity

### Compared files

- Upstream:
  - `yarn-project/aztec.js/src/utils/field_compressed_string.ts`
  - `yarn-project/aztec.js/src/utils/fee_juice.ts`
- Rust:
  - full workspace search across `src` and `crates/*`

### Concrete code deltas

- Upstream exports `readFieldCompressedString(...)`; no corresponding Rust symbol was found.
- Upstream exports `getFeeJuiceBalance(...)`; no corresponding Rust symbol was found.

### Planned work

1. Add these as small utility functions after the larger wallet/PXE/API deltas are settled.

---

## Release Order Based on Current Code Deltas

## Release 0.3.3: Finish Existing Runtime Gaps

Target files:
- `crates/pxe/src/kernel/oracle.rs`
- `crates/pxe/src/embedded_pxe.rs`
- `crates/wallet/src/base_wallet.rs`
- `crates/contract/src/contract.rs`
- `crates/node-client/src/node.rs`

Scope:
- close explicit PXE `not implemented yet` paths
- replace the TX constant data / expiration placeholders
- add `NO_WAIT` only if send result handling is upgraded
- finish gas/result extraction where Rust already has TODO markers
- add tests for the currently incomplete runtime paths

## Release 0.4.0: Wallet and Interaction Surface Parity

Target files:
- `crates/wallet/src/wallet.rs`
- `crates/contract/src/contract.rs`
- `crates/fee/src/fee_payment_method.rs`
- `crates/fee/src/*`

Scope:
- decide on wallet-level `batch(...)`
- decide on capabilities
- decide on upstream-style fee-method integration and gas-settings flow
- add any chosen interaction-option parity items

## Release 0.5.0: Messaging and Bridge Foundation

Target files:
- `crates/ethereum/src/lib.rs`
- `crates/ethereum/src/messaging.rs`

Scope:
- add `L1ToL2Message`, `L1Actor`, `L2Actor`
- add `is_l1_to_l2_message_ready(...)`
- add `wait_for_l1_to_l2_message_ready(...)`
- add `generate_claim_secret(...)`

## Release 0.6.0: Portal Managers and Remaining Public API Types

Target files:
- `crates/ethereum/*`
- `crates/core/*`
- `crates/node-client/*`

Scope:
- add L1 portal managers
- add block/tree/log/note/protocol wrappers still missing from the public API

## Release 0.7.0: Small Utilities and Cleanup

Target files:
- utility locations to be chosen
- `README.md`
- crate-level docs and re-exports

Scope:
- add `read_field_compressed_string(...)`
- add `get_fee_juice_balance(...)`
- document final API divergences that remain intentional

---

## Immediate Priorities

1. Finish the explicit PXE runtime gaps that already have Rust placeholders or explicit `not implemented yet` paths.
2. Decide the final Rust shape for wallet batching, capabilities, and no-wait send semantics before adding more surface area.
3. Build the messaging layer in `crates/ethereum` before attempting full bridge managers.
4. Add the missing public API wrapper/data types only after the runtime and wallet semantics are stable.
