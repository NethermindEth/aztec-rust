# aztec-rs Gap Analysis vs aztec.js

**Date:** 2026-04-06
**Reference:** `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js`
**Target:** `/Users/alexmetelli/source/aztec-rust`

---

## Executive Summary

The Rust SDK (`aztec-rs`) has established a solid foundation with core types, node client, contract interactions, wallet trait, account abstraction, and deployment scaffolding. However, significant gaps remain to reach feature parity with `aztec.js`. The biggest missing areas are:

1. **No real PXE-backed Wallet implementation** — only a `MockWallet` exists; no `BaseWallet` that connects to a running PXE over HTTP/JSON-RPC
2. **Fee payment strategies** — only types exist, no payment method implementations
3. **Authorization** — module is empty, no authwit computation or utilities
4. **L1-L2 messaging** — module is empty, no cross-chain helpers
5. **Ethereum/L1 portal integration** — entirely missing module
6. **Wallet capabilities system** — not implemented
7. **Key derivation** — no key generation/derivation utilities
8. **Deployment internals** — `publish_contract_class` and `publish_instance` are stubs
9. **Account deployment** — `DeployAccountMethod` only partially functional
10. **Signerless account** — not implemented
11. **Various utility functions** — authwit computation, fee juice balance, ABI checkers, etc.

---

## PXE (Private eXecution Environment) — Architectural Gap

This is the most significant structural gap in the Rust SDK and deserves its own section.

### How aztec.js relates to PXE

In the TypeScript ecosystem, **aztec.js does NOT directly depend on or expose PXE**. Instead:

1. **aztec.js** defines the `Wallet` interface — a high-level abstraction over all PXE operations
2. **`@aztec/wallet-sdk`** provides `BaseWallet`, which implements `Wallet` by wrapping a `PXE` instance
3. **`@aztec/pxe`** provides the actual PXE server/client implementation

The `Wallet` interface is the seam between aztec.js and PXE. Every PXE operation is exposed through Wallet methods:

| Wallet Method | Delegates to PXE |
|---------------|-------------------|
| `registerSender()` | `pxe.registerSender()` |
| `registerContract()` | `pxe.registerContract()` + `pxe.updateContract()` |
| `simulateTx()` | `pxe.simulateTx()` (private + public simulation) |
| `sendTx()` | `pxe.proveTx()` → `aztecNode.sendTx()` |
| `profileTx()` | `pxe.profileTx()` |
| `executeUtility()` | `pxe.executeUtility()` |
| `getPrivateEvents()` | `pxe.getPrivateEvents()` |
| `getContractMetadata()` | `pxe.getContractInstance()` + `aztecNode.getContract()` |
| `getContractClassMetadata()` | `pxe.getContractArtifact()` + `aztecNode.getContractClass()` |
| `getAddressBook()` | `pxe.getSenders()` |
| `getAccounts()` | `pxe.getRegisteredAccounts()` |
| `createAuthWit()` | Account abstraction layer |

### What aztec-rs has

- The `Wallet` **trait** is defined with all the right methods (mirrors the TS interface)
- A `MockWallet` for testing that returns canned responses

### What aztec-rs is missing

| Gap | Reference File | Priority |
|-----|----------------|----------|
| **`BaseWallet`** — real implementation wrapping a PXE connection | `wallet-sdk/src/base-wallet/base_wallet.ts` | **P0** |
| **PXE JSON-RPC client** — HTTP client to talk to a running PXE node | `pxe/src/client/` | **P0** |
| **`PXE` trait** — interface for PXE operations (simulate, prove, register, etc.) | `stdlib/src/interfaces/pxe.ts` | **P0** |
| **`createPXEClient()`** — factory to create HTTP-backed PXE client | `pxe/src/client/lazy/` | **P0** |
| PXE account registration (`registerAccount()`) | `pxe/src/pxe.ts` | P0 |
| PXE sender management (`removeSender()`) | `pxe/src/pxe.ts` | P1 |
| PXE contract class registration (`registerContractClass()`) | `pxe/src/pxe.ts` | P1 |
| PXE state queries (`getSyncedBlockHeader()`, `getContracts()`) | `pxe/src/pxe.ts` | P2 |

### Why this matters

Without a real `BaseWallet` + PXE client, **the Rust SDK cannot actually execute private transactions**. The `MockWallet` is useful for unit tests, but any real application needs to:

1. Connect to a PXE instance (local or remote)
2. Register accounts and contracts with PXE
3. Have PXE simulate and prove transactions (private execution happens in PXE)
4. Send proven transactions to the Aztec node

The current `HttpNodeClient` only talks to the **Aztec Node** (public state, block queries, tx submission). PXE is a separate service that handles all **private state and execution**.

### Reference files (outside aztec.js)

These files are in the broader `aztec-packages/yarn-project/` — not in aztec.js itself, but they're needed for a functional SDK:

- `/Users/alexmetelli/source/aztec-packages/yarn-project/wallet-sdk/src/base-wallet/base_wallet.ts` — BaseWallet implementation
- `/Users/alexmetelli/source/aztec-packages/yarn-project/pxe/src/pxe.ts` — PXE server implementation
- `/Users/alexmetelli/source/aztec-packages/yarn-project/pxe/src/client/` — PXE client (HTTP/JSON-RPC)
- `/Users/alexmetelli/source/aztec-packages/yarn-project/stdlib/src/interfaces/pxe.ts` — PXE interface definition

---

## Module-by-Module Comparison

### 1. Account Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| `Account` trait/interface | `src/account/account.ts` | `src/account.rs` | Implemented |
| `AccountContract` trait | `src/account/account_contract.ts` | `src/account.rs` | Implemented |
| `AuthorizationProvider` trait | `src/account/account.ts` | `src/account.rs` | Implemented |
| `BaseAccount` implementation | `src/account/account.ts` | `src/account.rs` (inline) | Implemented |
| `AccountWithSecretKey` | `src/account/account_with_secret_key.ts` | `src/account.rs` | Implemented |
| `SignerlessAccount` | `src/account/signerless_account.ts` | — | **Missing** |
| `getAccountContractAddress()` | `src/account/account_contract.ts` | — | **Missing** |
| `InitializationSpec` | — | `src/account.rs` | Rust-only extra |
| `Salt` type alias | `src/account/account.ts` | — | **Missing** (minor) |

**Reference files to port:**
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/account/signerless_account.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/account/account_contract.ts` (for `getAccountContractAddress`)

---

### 2. Contract Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| `Contract` (with `at`, `deploy`, `deployWithPublicKeys`) | `src/contract/contract.ts` | `src/contract.rs` | Partial — missing `deploy`/`deployWithPublicKeys` static methods |
| `ContractBase` (dynamic method creation) | `src/contract/contract_base.ts` | `src/contract.rs` | Partial — no dynamic method map |
| `ContractFunctionInteraction` | `src/contract/contract_function_interaction.ts` | `src/contract.rs` | Implemented |
| `BaseContractInteraction` | `src/contract/base_contract_interaction.ts` | `src/contract.rs` | Implemented |
| `BatchCall` | `src/contract/batch_call.ts` | `src/contract.rs` | Implemented |
| `DeployMethod` | `src/contract/deploy_method.ts` | `src/deployment.rs` | Partial — missing request/simulate/send internals |
| `getGasLimits()` | `src/contract/get_gas_limits.ts` | — | **Missing** |
| `waitForProven()` | `src/contract/wait_for_proven.ts` | — | **Missing** |
| `abiChecker()` | `src/contract/checker.ts` | — | **Missing** |
| Interaction options (full type system) | `src/contract/interaction_options.ts` | `src/wallet.rs` | Partial — simplified |
| `WaitOpts` with `dontThrowOnRevert` | `src/contract/wait_opts.ts` | `src/node.rs` | Partial — missing `dontThrowOnRevert`, `waitForStatus`, `ignoreDroppedReceiptsFor` |
| `NO_WAIT` constant | `src/contract/wait_opts.ts` | — | **Missing** |
| `ContractMethod` type (dynamic dispatch) | `src/contract/contract_base.ts` | — | **Missing** |
| `ContractStorageLayout` | `src/contract/contract_base.ts` | — | **Missing** |
| `profile()` method on interactions | `src/contract/contract_function_interaction.ts` | — | **Missing** |

**Reference files to port:**
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/contract/get_gas_limits.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/contract/wait_for_proven.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/contract/checker.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/contract/interaction_options.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/contract/wait_opts.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/contract/contract.ts` (for static deploy methods)
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/contract/contract_base.ts` (for dynamic method creation)

---

### 3. Wallet Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| `Wallet` trait/type | `src/wallet/wallet.ts` | `src/wallet.rs` | Implemented |
| `AccountManager` | `src/wallet/account_manager.ts` | `src/account.rs` | Implemented (different location) |
| `DeployAccountMethod` | `src/wallet/deploy_account_method.ts` | `src/account.rs` | Partial — stub internals |
| `MockWallet` | — | `src/wallet.rs` | Rust-only (good for testing) |
| `Aliased<T>` type | `src/wallet/wallet.ts` | `src/wallet.rs` | Implemented |
| `ContractMetadata` | `src/wallet/wallet.ts` | `src/wallet.rs` | Implemented |
| `ContractClassMetadata` | `src/wallet/wallet.ts` | `src/wallet.rs` | Implemented |
| `SimulateOptions` / `SendOptions` / etc. | `src/wallet/wallet.ts` | `src/wallet.rs` | Implemented |
| `AccountEntrypointMetaPaymentMethod` | `src/wallet/account_entrypoint_meta_payment_method.ts` | — | **Missing** |
| **Capabilities system** | `src/wallet/capabilities.ts` | — | **Missing** |
| `AppCapabilities` | `src/wallet/capabilities.ts` | — | **Missing** |
| `WalletCapabilities` | `src/wallet/capabilities.ts` | — | **Missing** |
| `requestCapabilities()` on Wallet | `src/wallet/wallet.ts` | — | **Missing** |
| `batch()` on Wallet | `src/wallet/wallet.ts` | — | **Missing** |
| `BatchedMethod` / `BatchResults` types | `src/wallet/wallet.ts` | — | **Missing** |
| Zod validation schemas | `src/wallet/wallet.ts` | — | N/A (Rust uses serde) |

**Reference files to port:**
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/wallet/capabilities.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/wallet/account_entrypoint_meta_payment_method.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/wallet/deploy_account_method.ts` (for complete internals)

---

### 4. Fee Payment Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| `Gas` / `GasFees` / `GasSettings` types | `src/fee/*` | `src/fee.rs` | Implemented |
| `FeePaymentMethod` trait | `src/fee/fee_payment_method.ts` | — | **Missing** |
| `PrivateFeePaymentMethod` | `src/fee/private_fee_payment_method.ts` | — | **Missing** |
| `PublicFeePaymentMethod` | `src/fee/public_fee_payment_method.ts` | — | **Missing** |
| `FeeJuicePaymentMethodWithClaim` | `src/fee/fee_juice_payment_method_with_claim.ts` | — | **Missing** |
| `SponsoredFeePaymentMethod` | `src/fee/sponsored_fee_payment.ts` | — | **Missing** |

**Reference files to port:**
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/fee/fee_payment_method.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/fee/private_fee_payment_method.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/fee/public_fee_payment_method.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/fee/fee_juice_payment_method_with_claim.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/fee/sponsored_fee_payment.ts`

---

### 5. Authorization Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| `CallAuthorizationRequest` | `src/authorization/call_authorization_request.ts` | — | **Missing** (module empty) |
| `computeAuthWitMessageHash()` | `src/utils/authwit.ts` | — | **Missing** |
| `computeInnerAuthWitHashFromAction()` | `src/utils/authwit.ts` | — | **Missing** |
| `lookupValidity()` | `src/utils/authwit.ts` | — | **Missing** |
| `SetPublicAuthwitContractInteraction` | `src/utils/authwit.ts` | — | **Missing** |
| `IntentInnerHash` type | `src/utils/authwit.ts` | `src/wallet.rs` | Partial — type exists but no computation |
| `CallIntent` type | `src/utils/authwit.ts` | `src/wallet.rs` | Partial — type exists |

**Reference files to port:**
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/authorization/call_authorization_request.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/utils/authwit.ts`

---

### 6. Deployment Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| `ContractDeployer` | `src/deployment/contract_deployer.ts` | `src/deployment.rs` | Implemented |
| `publishContractClass()` | `src/deployment/publish_class.ts` | `src/deployment.rs` | **Stub** — returns "not yet implemented" |
| `publishInstance()` | `src/deployment/publish_instance.ts` | `src/deployment.rs` | **Stub** — returns "not yet implemented" |
| `DeployOptions` | `src/contract/deploy_method.ts` | `src/deployment.rs` | Implemented |
| Address derivation from salt/deployer | `src/contract/deploy_method.ts` | — | **Missing** |

**Reference files to port:**
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/deployment/publish_class.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/deployment/publish_instance.ts`

---

### 7. Ethereum / L1 Portal Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| Entire module | `src/ethereum/portal_manager.ts` | — | **Missing** (no module) |
| `L1TokenManager` | `src/ethereum/portal_manager.ts` | — | **Missing** |
| `L1FeeJuicePortalManager` | `src/ethereum/portal_manager.ts` | — | **Missing** |
| `L1ToL2TokenPortalManager` | `src/ethereum/portal_manager.ts` | — | **Missing** |
| `L1TokenPortalManager` | `src/ethereum/portal_manager.ts` | — | **Missing** |
| `L2Claim` / `L2AmountClaim` types | `src/ethereum/portal_manager.ts` | — | **Missing** |
| `generateClaimSecret()` | `src/ethereum/portal_manager.ts` | — | **Missing** |

**Reference files to port:**
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/ethereum/portal_manager.ts`

---

### 8. L1-L2 Messaging Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| `waitForL1ToL2MessageReady()` | `src/utils/cross_chain.ts` | — | **Missing** (module empty) |
| `isL1ToL2MessageReady()` | `src/utils/cross_chain.ts` | — | **Missing** |
| L1ToL2Message / L1Actor / L2Actor types | re-exported from `@aztec/stdlib` | — | **Missing** |

**Reference files to port:**
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/utils/cross_chain.ts`

---

### 9. Events Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| `PublicEvent<T>` / `PublicEventMetadata` | `src/api/events.ts` | `src/events.rs` | Implemented |
| `PublicEventFilter` | `src/api/events.ts` | `src/events.rs` | Implemented |
| `GetPublicEventsResult` | `src/api/events.ts` | `src/events.rs` | Implemented |
| `getPublicEvents()` helper function | `src/api/events.ts` | `src/events.rs` | Partial — decoding logic exists |
| `PrivateEvent<T>` type | `src/wallet/wallet.ts` | `src/wallet.rs` | Implemented |
| `PrivateEventFilter` | `src/wallet/wallet.ts` | `src/wallet.rs` | Implemented |
| `EventMetadataDefinition` | `src/wallet/wallet.ts` | `src/wallet.rs` | Implemented |

**Status: Mostly complete.** Events are one of the stronger areas of the Rust port.

---

### 10. Node Client Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| `createAztecNodeClient()` | `src/utils/node.ts` | `src/node.rs` | Implemented |
| `waitForNode()` | `src/utils/node.ts` | `src/node.rs` | Implemented |
| `waitForTx()` | `src/utils/node.ts` | `src/node.rs` | Implemented |
| `AztecNode` trait | `src/utils/node.ts` | `src/node.rs` | Implemented |
| `NodeInfo` | — | `src/node.rs` | Implemented |
| `HttpNodeClient` | — | `src/node.rs` | Implemented |
| `PublicLogFilter` / `PublicLog` | — | `src/node.rs` | Implemented |
| `WaitOpts` | `src/contract/wait_opts.ts` | `src/node.rs` | Partial (missing fields) |

**Status: Mostly complete.** Missing some `WaitOpts` fields (`dontThrowOnRevert`, `waitForStatus`, `ignoreDroppedReceiptsFor`).

---

### 11. Transaction Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| `TxHash` | re-exported | `src/tx.rs` | Implemented |
| `TxStatus` | re-exported | `src/tx.rs` | Implemented |
| `TxReceipt` | re-exported | `src/tx.rs` | Implemented |
| `TxExecutionResult` | re-exported | `src/tx.rs` | Implemented |
| `FunctionCall` | re-exported | `src/tx.rs` | Implemented |
| `ExecutionPayload` | re-exported | `src/tx.rs` | Implemented |
| `AuthWitness` | re-exported | `src/tx.rs` | Implemented |
| `Capsule` | re-exported | `src/tx.rs` | Implemented |
| `HashedValues` | re-exported | `src/tx.rs` | Implemented |
| `mergeExecutionPayloads()` | re-exported | — | **Missing** |
| `GlobalVariables` | re-exported | — | **Missing** |
| `Tx` (full transaction object) | re-exported | — | **Missing** |
| `TxExecutionRequest` | re-exported | `src/account.rs` | Implemented |

**Status: Mostly complete.** A few missing utility types.

---

### 12. Types / Fields Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| `Fr` | re-exported | `src/types.rs` | Implemented |
| `Fq` | re-exported | `src/types.rs` | Implemented |
| `Point` | re-exported | `src/types.rs` | Implemented |
| `GrumpkinScalar` | re-exported | `src/types.rs` | Implemented |
| `AztecAddress` | re-exported | `src/types.rs` | Implemented |
| `EthAddress` | re-exported | `src/types.rs` | Implemented |
| `CompleteAddress` | re-exported | `src/types.rs` | Implemented |
| `PublicKeys` | re-exported | `src/types.rs` | Implemented |
| `ContractInstance` | re-exported | `src/types.rs` | Implemented |
| `ContractInstanceWithAddress` | re-exported | `src/types.rs` | Implemented |
| `BlockNumber` | re-exported | — | **Missing** (minor) |

**Status: Complete for core types.**

---

### 13. ABI Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| `ContractArtifact` / `FunctionArtifact` | re-exported | `src/abi.rs` | Implemented |
| `FunctionSelector` / `EventSelector` | re-exported | `src/abi.rs` | Implemented |
| `FunctionType` | re-exported | `src/abi.rs` | Implemented |
| `AbiType` / `AbiValue` / `AbiParameter` | — | `src/abi.rs` | Implemented |
| `encodeArguments()` | re-exported | — | **Missing** |
| `decodeFromAbi()` | re-exported | — | **Missing** |
| `loadContractArtifact()` | re-exported | `src/abi.rs` (`from_json`) | Implemented (different name) |
| `contractArtifactToBuffer()` | re-exported | — | **Missing** |
| `contractArtifactFromBuffer()` | re-exported | — | **Missing** |
| `NoteSelector` | re-exported | — | **Missing** |
| `FunctionSelector.from_name()` | `src/api/abi.ts` | `src/abi.rs` | **Stub** — unimplemented |
| Type checkers (`isAddressStruct`, etc.) | `src/utils/abi_types.ts` | — | **Missing** |
| Type converters (`FieldLike`, `AztecAddressLike`, etc.) | `src/utils/abi_types.ts` | — | **Missing** |

**Reference files to port:**
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/utils/abi_types.ts`

---

### 14. Keys Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| Entire module | `src/api/keys.ts` | — | **Missing** (no module) |
| `generatePublicKey()` | `src/utils/pub_key.ts` | — | **Missing** |
| `deriveKeys()` | re-exported from stdlib | — | **Missing** |
| `deriveMasterIncomingViewingSecretKey()` | re-exported | — | **Missing** |
| `deriveMasterNullifierHidingKey()` | re-exported | — | **Missing** |
| `computeAppNullifierHidingKey()` | re-exported | — | **Missing** |

**Reference files to port:**
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/utils/pub_key.ts`
- Key derivation functions from `@aztec/stdlib`

---

### 15. Crypto Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| `computeSecretHash()` | re-exported from stdlib | — | **Missing** |

---

### 16. Protocol Module

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| Entire module | `src/api/protocol.ts` | — | **Missing** (no module) |
| `ProtocolContractAddress` | re-exported | — | **Missing** |
| Protocol contract wrappers | re-exported | — | **Missing** |

---

### 17. Block / Trees / Log / Note Modules

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| Block types (`L2Block`, `Body`) | `src/api/block.ts` | — | **Missing** |
| `getTimestampRangeForEpoch()` | `src/api/block.ts` | — | **Missing** |
| Tree types (`SiblingPath`, `MerkleTreeId`) | `src/api/trees.ts` | — | **Missing** |
| Log types (`LogId`, `LogFilter`) | `src/api/log.ts` | — | **Missing** |
| Note types (`Comparator`, `Note`) | `src/api/note.ts` | — | **Missing** |

---

### 18. Utilities

| Feature | aztec.js | aztec-rs | Status |
|---------|----------|----------|--------|
| `getFeeJuiceBalance()` | `src/utils/fee_juice.ts` | — | **Missing** |
| `readFieldCompressedString()` | `src/utils/field_compressed_string.ts` | — | **Missing** |
| `FieldLike` / `AztecAddressLike` / etc. | `src/utils/abi_types.ts` | — | **Missing** |

**Reference files to port:**
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/utils/fee_juice.ts`
- `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/src/utils/field_compressed_string.ts`

---

## Priority-Ordered Implementation Roadmap

### P0 — Critical (needed for basic contract deployment and interaction)

0. **PXE client + BaseWallet** — Without these, **no real private execution is possible**
   - Define a `Pxe` trait mirroring the TS `PXE` interface
   - Implement an HTTP/JSON-RPC PXE client (`create_pxe_client()`)
   - Implement `BaseWallet` that wraps PXE + AztecNode and implements the `Wallet` trait
   - Reference: `wallet-sdk/src/base-wallet/base_wallet.ts`, `pxe/src/client/`, `stdlib/src/interfaces/pxe.ts`

1. **Fee payment methods** — Without these, no real transaction can be sent
   - `FeePaymentMethod` trait
   - `FeeJuicePaymentMethodWithClaim`
   - `SponsoredFeePaymentMethod`
   - Files: `src/fee/fee_payment_method.ts`, `src/fee/fee_juice_payment_method_with_claim.ts`, `src/fee/sponsored_fee_payment.ts`

2. **Authorization module** — Required for any multi-party interaction
   - `computeAuthWitMessageHash()`
   - `computeInnerAuthWitHashFromAction()`
   - `CallAuthorizationRequest`
   - Files: `src/utils/authwit.ts`, `src/authorization/call_authorization_request.ts`

3. **Deployment internals** — Complete the stubs
   - `publishContractClass()` — actual implementation
   - `publishInstance()` — actual implementation
   - Address derivation logic
   - Files: `src/deployment/publish_class.ts`, `src/deployment/publish_instance.ts`

4. **`getGasLimits()`** — Needed for proper gas estimation before sending
   - File: `src/contract/get_gas_limits.ts`

5. **`mergeExecutionPayloads()`** — Used by batch calls and deployment
   - From `@aztec/stdlib`

### P1 — High (needed for account management and full workflow)

6. **Key derivation utilities**
   - `generatePublicKey()`
   - `deriveKeys()`
   - `deriveMasterIncomingViewingSecretKey()`
   - Files: `src/utils/pub_key.ts`, stdlib key derivation

7. **`SignerlessAccount`** — Used for fee-sponsored transactions
   - File: `src/account/signerless_account.ts`

8. **`AccountEntrypointMetaPaymentMethod`** — Fee payment during account deployment
   - File: `src/wallet/account_entrypoint_meta_payment_method.ts`

9. **`getAccountContractAddress()`** — Compute account address before deployment
   - File: `src/account/account_contract.ts`

10. **Complete `WaitOpts`** — Add missing fields (`dontThrowOnRevert`, `waitForStatus`, `ignoreDroppedReceiptsFor`)
    - File: `src/contract/wait_opts.ts`

### P2 — Medium (needed for L1-L2 workflows and DeFi)

11. **L1-L2 messaging**
    - `waitForL1ToL2MessageReady()`
    - `isL1ToL2MessageReady()`
    - File: `src/utils/cross_chain.ts`

12. **Ethereum portal managers**
    - `L1TokenManager`
    - `L1FeeJuicePortalManager`
    - `L1ToL2TokenPortalManager`
    - `generateClaimSecret()`
    - File: `src/ethereum/portal_manager.ts`

13. **`waitForProven()`** — Wait for transaction to be proven on L1
    - File: `src/contract/wait_for_proven.ts`

14. **Private/Public fee payment methods** (deprecated but still used)
    - `PrivateFeePaymentMethod`
    - `PublicFeePaymentMethod`
    - Files: `src/fee/private_fee_payment_method.ts`, `src/fee/public_fee_payment_method.ts`

### P3 — Lower (polish and completeness)

15. **Wallet capabilities system**
    - `AppCapabilities`, `WalletCapabilities`, `requestCapabilities()`
    - Capability types and granted types
    - File: `src/wallet/capabilities.ts`

16. **`batch()` on Wallet** — Batched wallet method calls
    - File: `src/wallet/wallet.ts`

17. **ABI utilities**
    - `encodeArguments()`, `decodeFromAbi()`
    - `contractArtifactToBuffer()`, `contractArtifactFromBuffer()`
    - `FunctionSelector.from_name()` (currently unimplemented)
    - ABI type checkers and converters
    - Files: `src/utils/abi_types.ts`, `@aztec/stdlib`

18. **`Contract.deploy()` / `Contract.deployWithPublicKeys()` static methods**
    - File: `src/contract/contract.ts`

19. **Protocol contract addresses and wrappers**
    - `ProtocolContractAddress`
    - Protocol contract wrapper types
    - File: `src/api/protocol.ts`

20. **Misc utilities**
    - `getFeeJuiceBalance()`
    - `readFieldCompressedString()`
    - `computeSecretHash()`
    - `abiChecker()`
    - `getTimestampRangeForEpoch()`
    - Files: `src/utils/fee_juice.ts`, `src/utils/field_compressed_string.ts`, `src/contract/checker.ts`

21. **Block / Tree / Log / Note types** — Mostly re-exports from stdlib
    - Files: `src/api/block.ts`, `src/api/trees.ts`, `src/api/log.ts`, `src/api/note.ts`

22. **`profile()` on contract interactions** — Gas profiling support
    - File: `src/contract/contract_function_interaction.ts`

---

## Summary Statistics

| Category | aztec.js items | aztec-rs implemented | Gap |
|----------|---------------|---------------------|-----|
| **PXE + BaseWallet** | ~8 | ~0 | **8** |
| Core types | ~15 | ~15 | 0 |
| Node client | ~8 | ~7 | 1 |
| ABI | ~15 | ~10 | 5 |
| Contract interaction | ~18 | ~10 | 8 |
| Wallet | ~20 | ~12 | 8 |
| Account | ~8 | ~5 | 3 |
| Deployment | ~5 | ~2 | 3 |
| Fee payment | ~6 | ~1 (types only) | 5 |
| Authorization | ~6 | ~0 | 6 |
| Ethereum/L1 | ~6 | ~0 | 6 |
| L1-L2 messaging | ~4 | ~0 | 4 |
| Keys/Crypto | ~6 | ~0 | 6 |
| Events | ~8 | ~7 | 1 |
| Protocol | ~7 | ~0 | 7 |
| Utilities | ~8 | ~0 | 8 |
| Block/Tree/Log/Note | ~8 | ~0 | 8 |
| **Total** | **~156** | **~69** | **~87** |

**Approximate feature parity: ~44%**

> **Note:** The PXE + BaseWallet gap is the most critical. Without it, the SDK can only talk to the Aztec Node (public state). All private execution — which is the core value proposition of Aztec — requires a PXE connection. The `Wallet` trait is defined correctly, but there is no concrete implementation that connects to a real PXE instance.

---

## Complete List of aztec.js Source Files to Port

### Must port (PXE + Wallet infrastructure — outside aztec.js):

These files are in the broader `yarn-project/` monorepo, not in `aztec.js/` itself:

| # | File (relative to `yarn-project/`) | Priority |
|---|------|----------|
| 0a | `stdlib/src/interfaces/pxe.ts` | P0 |
| 0b | `pxe/src/client/` (PXE HTTP/JSON-RPC client) | P0 |
| 0c | `wallet-sdk/src/base-wallet/base_wallet.ts` | P0 |
| 0d | `pxe/src/pxe.ts` (reference for PXE operations) | P0 |

### Must port from aztec.js (implementation logic):
| # | File | Priority |
|---|------|----------|
| 1 | `src/fee/fee_payment_method.ts` | P0 |
| 2 | `src/fee/fee_juice_payment_method_with_claim.ts` | P0 |
| 3 | `src/fee/sponsored_fee_payment.ts` | P0 |
| 4 | `src/utils/authwit.ts` | P0 |
| 5 | `src/authorization/call_authorization_request.ts` | P0 |
| 6 | `src/deployment/publish_class.ts` | P0 |
| 7 | `src/deployment/publish_instance.ts` | P0 |
| 8 | `src/contract/get_gas_limits.ts` | P0 |
| 9 | `src/utils/pub_key.ts` | P1 |
| 10 | `src/account/signerless_account.ts` | P1 |
| 11 | `src/wallet/account_entrypoint_meta_payment_method.ts` | P1 |
| 12 | `src/account/account_contract.ts` | P1 |
| 13 | `src/contract/wait_opts.ts` | P1 |
| 14 | `src/utils/cross_chain.ts` | P2 |
| 15 | `src/ethereum/portal_manager.ts` | P2 |
| 16 | `src/contract/wait_for_proven.ts` | P2 |
| 17 | `src/fee/private_fee_payment_method.ts` | P2 |
| 18 | `src/fee/public_fee_payment_method.ts` | P2 |
| 19 | `src/wallet/capabilities.ts` | P3 |
| 20 | `src/utils/abi_types.ts` | P3 |
| 21 | `src/contract/contract.ts` | P3 |
| 22 | `src/contract/contract_base.ts` | P3 |
| 23 | `src/contract/interaction_options.ts` | P3 |
| 24 | `src/contract/checker.ts` | P3 |
| 25 | `src/utils/fee_juice.ts` | P3 |
| 26 | `src/utils/field_compressed_string.ts` | P3 |
| 27 | `src/wallet/deploy_account_method.ts` | P3 |

### Re-export only (types from @aztec/stdlib — need Rust equivalents):
| # | File | Priority |
|---|------|----------|
| 28 | `src/api/protocol.ts` | P3 |
| 29 | `src/api/block.ts` | P3 |
| 30 | `src/api/trees.ts` | P3 |
| 31 | `src/api/log.ts` | P3 |
| 32 | `src/api/note.ts` | P3 |
| 33 | `src/api/keys.ts` | P1 |
| 34 | `src/api/crypto.ts` | P3 |
| 35 | `src/api/messaging.ts` | P2 |

All file paths above are relative to `/Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js/`.
