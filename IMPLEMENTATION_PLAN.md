# aztec-rs Incremental Implementation Plan

**Based on:** [GAP_ANALYSIS.md](./GAP_ANALYSIS.md)
**Current feature parity:** ~44% (~69/156 items)
**Goal:** Reach functional parity with aztec.js through incremental, releasable steps

---

## Versioning Strategy

Each step below produces a publishable release. We follow semver:
- **0.2.x** — Core infrastructure (PXE, fees, auth)
- **0.3.x** — Account & deployment completeness
- **0.4.x** — L1-L2 bridge support
- **0.5.x** — Polish, capabilities, utilities
- **1.0.0** — Full feature parity with aztec.js

---

## Step 1: PXE Trait & Client (`aztec-pxe-client`) — Release 0.2.0

**Why first:** Without PXE connectivity, no private execution is possible. This is the single biggest architectural gap.

### Deliverables
- [ ] Define `Pxe` trait in `aztec-pxe-client` mirroring the TS `PXE` interface
  - `simulate_tx()`, `prove_tx()`, `send_tx()`
  - `register_account()`, `get_registered_accounts()`
  - `register_sender()`, `get_senders()`
  - `register_contract()`, `update_contract()`, `get_contracts()`
  - `get_contract_instance()`, `get_contract_artifact()`
  - `execute_utility()`
  - `get_private_events()`
  - `profile_tx()`
  - `get_synced_block_header()`
- [ ] Implement `HttpPxeClient` — HTTP/JSON-RPC client to a running PXE node
  - Reuse `aztec-rpc` infrastructure (JSON-RPC request/response types)
  - Connection URL configuration, retry logic, timeout handling
- [ ] `create_pxe_client(url)` factory function
- [ ] Integration test against a local PXE node (can be gated behind a feature flag)

### Reference files
- `yarn-project/stdlib/src/interfaces/pxe.ts`
- `yarn-project/pxe/src/client/`
- `yarn-project/pxe/src/pxe.ts`

### Release notes
> Introduces the `Pxe` trait and `HttpPxeClient`, enabling Rust applications to connect to Aztec PXE nodes for private execution.

---

## Step 2: BaseWallet Implementation (`aztec-wallet`) — Release 0.2.1

**Why next:** With PXE client in hand, we can build a real wallet that replaces `MockWallet` for production use.

### Deliverables
- [ ] `BaseWallet` struct that wraps a `Pxe` + `AztecNode` client and implements the `Wallet` trait
  - Route `simulate_tx` / `send_tx` / `prove_tx` through PXE
  - Route `register_contract` / `register_sender` through PXE
  - Route block/tx queries through AztecNode
  - `create_auth_wit()` via the account abstraction layer
  - `get_private_events()` via PXE
  - `get_contract_metadata()` via PXE + node
  - `get_contract_class_metadata()` via PXE + node
  - `get_address_book()` / `get_accounts()` via PXE
- [ ] `create_wallet(pxe, account)` convenience constructor
- [ ] Unit tests with mock PXE trait implementation

### Reference files
- `yarn-project/wallet-sdk/src/base-wallet/base_wallet.ts`

### Release notes
> Adds `BaseWallet` — a production-ready `Wallet` implementation that connects to a PXE node. The SDK can now execute private transactions.

---

## Step 3: Fee Payment Methods (`aztec-fee`) — Release 0.2.2

**Why next:** Transactions require fee payment. Without payment methods, you can construct but not send transactions.

### Deliverables
- [ ] `FeePaymentMethod` trait
  - `get_asset()` -> `AztecAddress`
  - `get_fee_payer()` -> `AztecAddress`
  - `get_fee_execution_payload()` -> `ExecutionPayload`
- [ ] `SponsoredFeePaymentMethod` — simplest strategy (someone else pays)
- [ ] `FeeJuicePaymentMethodWithClaim` — pay with fee juice using an L1 claim
  - Requires `FunctionCall` construction for the fee juice contract
- [ ] `NativeFeePaymentMethod` — pay with native fee juice (no claim)
- [ ] Tests for each payment method

### Reference files
- `aztec.js/src/fee/fee_payment_method.ts`
- `aztec.js/src/fee/sponsored_fee_payment.ts`
- `aztec.js/src/fee/fee_juice_payment_method_with_claim.ts`

### Release notes
> Introduces fee payment strategies: `SponsoredFeePaymentMethod`, `FeeJuicePaymentMethodWithClaim`, and `NativeFeePaymentMethod`. Transactions can now be sent with proper fee payment.

---

## Step 4: Authorization Witnesses (`aztec-core`, `aztec-wallet`) — Release 0.2.3

**Why next:** AuthWit is required for any multi-party interaction (token approvals, DeFi, etc.).

### Deliverables
- [ ] `compute_auth_wit_message_hash(caller, chain_id, version, action)` in `aztec-core`
- [ ] `compute_inner_auth_wit_hash_from_action(caller, action)` in `aztec-core`
- [ ] `CallAuthorizationRequest` struct and flow in `aztec-account`
- [ ] `SetPublicAuthWitContractInteraction` in `aztec-contract`
- [ ] `lookup_validity()` utility — check if an authwit is valid on-chain
- [ ] Wire `create_auth_wit()` in `BaseWallet` to use the account's `AuthorizationProvider`
- [ ] Tests for authwit hash computation and validity checks

### Reference files
- `aztec.js/src/utils/authwit.ts`
- `aztec.js/src/authorization/call_authorization_request.ts`

### Release notes
> Implements authorization witnesses (authwit) — hash computation, validity checking, and public authwit interactions. Enables multi-party workflows like token approvals.

---

## Step 5: Deployment Internals (`aztec-contract`) — Release 0.2.4

**Why next:** `publishContractClass` and `publishInstance` are stubs. Real contract deployment requires these.

### Deliverables
- [ ] `publish_contract_class()` — full implementation
  - Pack bytecode, compute artifact hash
  - Construct registration function call
  - Return `ContractFunctionInteraction`
- [ ] `publish_instance()` — full implementation
  - Derive address from salt/deployer/contract class
  - Construct deployment function call
  - Return `ContractFunctionInteraction`
- [ ] Address derivation: `compute_contract_address_from_instance()`
- [ ] `merge_execution_payloads()` utility in `aztec-core`
- [ ] `get_gas_limits()` — estimate gas for a transaction
- [ ] Complete `DeployMethod` internals (request/simulate/send flow)
- [ ] Tests for address derivation and deployment flow

### Reference files
- `aztec.js/src/deployment/publish_class.ts`
- `aztec.js/src/deployment/publish_instance.ts`
- `aztec.js/src/contract/get_gas_limits.ts`

### Release notes
> Completes contract deployment: `publish_contract_class()`, `publish_instance()`, address derivation, and gas estimation. Contracts can now be fully deployed from Rust.

---

## Step 6: ABI Encoding & FunctionSelector (`aztec-core`) — Release 0.2.5

**Why next:** Many downstream features need proper ABI encode/decode and `FunctionSelector::from_name()`.

### Deliverables
- [ ] `encode_arguments(params, args)` — encode typed arguments to field elements
- [ ] `decode_from_abi(params, fields)` — decode field elements back to typed values
- [ ] `FunctionSelector::from_name(name)` — compute selector from function name (currently a stub)
- [ ] `NoteSelector` type
- [ ] ABI type checkers: `is_address_struct()`, `is_eth_address_struct()`, etc.
- [ ] Type converters: `FieldLike`, `AztecAddressLike`, `EthAddressLike`
- [ ] `contract_artifact_to_buffer()` / `contract_artifact_from_buffer()` for serialization
- [ ] Tests for encode/decode round-trips

### Reference files
- `aztec.js/src/utils/abi_types.ts`

### Release notes
> Adds ABI encoding/decoding utilities, `FunctionSelector::from_name()`, type checkers, and artifact serialization.

---

## Step 7: Key Derivation & Crypto (`aztec-crypto`) — Release 0.3.0

**Why next:** Account creation and management requires key derivation. This unlocks the full account lifecycle.

### Deliverables
- [ ] `generate_public_key(secret_key)` — derive public key from secret
- [ ] `derive_keys(secret)` — derive the full key set (nullifier, incoming viewing, outgoing viewing, tagging)
- [ ] `derive_master_incoming_viewing_secret_key(secret)`
- [ ] `derive_master_nullifier_hiding_key(secret)`
- [ ] `compute_app_nullifier_hiding_key(master_key, app)`
- [ ] `compute_secret_hash(secret)` — Pedersen hash for secret notes
- [ ] Tests for key derivation consistency with TS implementation

### Reference files
- `aztec.js/src/utils/pub_key.ts`
- stdlib key derivation functions

### Release notes
> Implements key derivation and cryptographic utilities. Enables full account lifecycle — from secret key to deployed account.

---

## Step 8: Account Completeness (`aztec-account`) — Release 0.3.1

**Why next:** With keys and deployment ready, we can complete the account module.

### Deliverables
- [ ] `SignerlessAccount` — account that requires no signing (for fee-sponsored tx)
- [ ] `get_account_contract_address(contract, secret_key, salt)` — compute address before deploying
- [ ] `AccountEntrypointMetaPaymentMethod` — fee payment during account deployment
- [ ] Complete `DeployAccountMethod` internals (currently partial)
  - Wire up PXE account registration
  - Proper fee handling during account deploy
- [ ] `Salt` type alias
- [ ] Tests for signerless account and address pre-computation

### Reference files
- `aztec.js/src/account/signerless_account.ts`
- `aztec.js/src/account/account_contract.ts`
- `aztec.js/src/wallet/account_entrypoint_meta_payment_method.ts`
- `aztec.js/src/wallet/deploy_account_method.ts`

### Release notes
> Completes account module: `SignerlessAccount`, address pre-computation, and deployment fee handling. All account types are now supported.

---

## Step 9: Contract Interaction Completeness (`aztec-contract`) — Release 0.3.2

**Why next:** Fill remaining gaps in contract interactions for a fully ergonomic developer experience.

### Deliverables
- [ ] Complete `WaitOpts` — add `dont_throw_on_revert`, `wait_for_status`, `ignore_dropped_receipts_for`
- [ ] `NO_WAIT` constant
- [ ] `wait_for_proven()` — poll until tx is proven on L1
- [ ] `profile()` method on `ContractFunctionInteraction` — gas profiling
- [ ] `Contract::deploy()` / `Contract::deploy_with_public_keys()` static methods
- [ ] `ContractStorageLayout` type
- [ ] `abi_checker()` — validate ABI compatibility
- [ ] Full `InteractionOptions` type with all fields from TS
- [ ] Tests for wait options and proven status polling

### Reference files
- `aztec.js/src/contract/wait_opts.ts`
- `aztec.js/src/contract/wait_for_proven.ts`
- `aztec.js/src/contract/contract.ts`
- `aztec.js/src/contract/checker.ts`
- `aztec.js/src/contract/interaction_options.ts`

### Release notes
> Completes contract interactions: `wait_for_proven()`, gas profiling, static deploy methods, and full wait/interaction options.

---

## Step 10: L1-L2 Messaging (`aztec-ethereum`) — Release 0.4.0

**Why next:** Enables cross-chain workflows — bridging assets between Ethereum and Aztec.

### Deliverables
- [ ] `wait_for_l1_to_l2_message_ready(pxe, message_hash)` — poll until message is consumable
- [ ] `is_l1_to_l2_message_ready(pxe, message_hash)` — one-shot check
- [ ] `L1Actor` / `L2Actor` types
- [ ] `L1ToL2Message` type
- [ ] Tests with mock PXE for message readiness

### Reference files
- `aztec.js/src/utils/cross_chain.ts`

### Release notes
> Adds L1-to-L2 message utilities. Applications can now wait for cross-chain messages to become consumable on Aztec.

---

## Step 11: Ethereum Portal Managers (`aztec-ethereum`) — Release 0.4.1

**Why next:** Builds on messaging to provide the full L1 bridge experience.

### Deliverables
- [ ] `L1TokenManager` — manage ERC-20 tokens on L1 for portal interactions
- [ ] `L1FeeJuicePortalManager` — bridge fee juice from L1 to L2
- [ ] `L1ToL2TokenPortalManager` — bridge arbitrary ERC-20 tokens L1 -> L2
- [ ] `L1TokenPortalManager` — base portal manager
- [ ] `generate_claim_secret()` — random secret + hash for portal claims
- [ ] `L2Claim` / `L2AmountClaim` types
- [ ] Requires an Ethereum JSON-RPC client (e.g., `ethers-rs` or `alloy` integration)
- [ ] Tests for claim secret generation and portal interactions

### Reference files
- `aztec.js/src/ethereum/portal_manager.ts`

### Release notes
> Introduces Ethereum portal managers for bridging assets between L1 and Aztec L2. Supports fee juice and arbitrary ERC-20 token bridging.

---

## Step 12: Private & Public Fee Payment Methods (`aztec-fee`) — Release 0.4.2

**Why next:** Completes the fee payment module with all strategies available in aztec.js.

### Deliverables
- [ ] `PrivateFeePaymentMethod` — pay fees via a private token swap
- [ ] `PublicFeePaymentMethod` — pay fees via a public token swap
- [ ] Tests for both payment methods

### Reference files
- `aztec.js/src/fee/private_fee_payment_method.ts`
- `aztec.js/src/fee/public_fee_payment_method.ts`

### Release notes
> Adds private and public fee payment methods for paying transaction fees via token swaps through a fee payment contract.

---

## Step 13: Wallet Capabilities & Batching (`aztec-wallet`) — Release 0.5.0

**Why next:** Capabilities system enables permission-gated wallet features. Batching enables efficient multi-call transactions.

### Deliverables
- [ ] `WalletCapabilities` / `AppCapabilities` types
- [ ] `request_capabilities()` on `Wallet` trait
- [ ] Capability types: `CapabilityType`, `GrantedCapability`
- [ ] `batch()` method on `Wallet` — batch multiple calls into one tx
- [ ] `BatchedMethod` / `BatchResults` types
- [ ] Tests for capability granting and batch execution

### Reference files
- `aztec.js/src/wallet/capabilities.ts`

### Release notes
> Adds wallet capabilities system and transaction batching. Applications can request permissions and bundle multiple calls into a single transaction.

---

## Step 14: Protocol Contracts & Block/Tree/Log/Note Types — Release 0.5.1

**Why next:** Fill in remaining type definitions for completeness.

### Deliverables
- [ ] `ProtocolContractAddress` enum/constants
- [ ] Protocol contract wrapper types
- [ ] `L2Block` / `Body` types
- [ ] `get_timestamp_range_for_epoch()`
- [ ] `SiblingPath` / `MerkleTreeId` tree types
- [ ] `LogId` / `LogFilter` types (extend existing `PublicLogFilter`)
- [ ] `Note` / `Comparator` types
- [ ] `BlockNumber` type alias
- [ ] `GlobalVariables` type
- [ ] `Tx` full transaction object type

### Reference files
- `aztec.js/src/api/protocol.ts`
- `aztec.js/src/api/block.ts`
- `aztec.js/src/api/trees.ts`
- `aztec.js/src/api/log.ts`
- `aztec.js/src/api/note.ts`

### Release notes
> Adds protocol contract addresses, block/tree/log/note types. The SDK now exposes the complete Aztec type system.

---

## Step 15: Utilities & Final Polish — Release 0.5.2

**Why next:** Last batch of missing utilities to reach full parity.

### Deliverables
- [ ] `get_fee_juice_balance(wallet, address)` — query fee juice balance
- [ ] `read_field_compressed_string(fields)` — decode compressed strings from field elements
- [ ] `ContractMethod` type (dynamic dispatch for contract methods)
- [ ] Dynamic method map on `ContractBase` (if feasible in Rust's type system; may require macro approach)
- [ ] Ensure all public API items are re-exported from the `aztec-rs` umbrella crate
- [ ] Documentation pass: ensure all public types/traits have doc comments
- [ ] Full integration test suite against a local Aztec sandbox

### Reference files
- `aztec.js/src/utils/fee_juice.ts`
- `aztec.js/src/utils/field_compressed_string.ts`
- `aztec.js/src/contract/contract_base.ts`

### Release notes
> Adds remaining utility functions and ensures full public API coverage. All aztec.js functionality is now available in Rust.

---

## Step 16: 1.0 Release — Release 1.0.0

### Deliverables
- [ ] End-to-end test: deploy account, deploy contract, send private tx, send public tx, read events
- [ ] End-to-end test: L1 bridge deposit, wait for message, consume on L2
- [ ] End-to-end test: authwit approval flow
- [ ] Performance benchmarks vs aztec.js for common operations
- [ ] API stability review — ensure no breaking changes needed
- [ ] Publish all workspace crates to crates.io
- [ ] Migration guide from aztec.js to aztec-rs

### Release notes
> First stable release of aztec-rs. Full feature parity with aztec.js. All workspace crates published to crates.io.

---

## Summary Table

| Step | Scope | Version | Crates Modified | Est. Parity |
|------|-------|---------|-----------------|-------------|
| 1 | PXE trait & HTTP client | 0.2.0 | `pxe-client`, `rpc` | ~49% |
| 2 | BaseWallet | 0.2.1 | `wallet` | ~54% |
| 3 | Fee payment methods | 0.2.2 | `fee` | ~58% |
| 4 | Authorization witnesses | 0.2.3 | `core`, `wallet`, `account`, `contract` | ~62% |
| 5 | Deployment internals | 0.2.4 | `contract`, `core` | ~67% |
| 6 | ABI encoding & selectors | 0.2.5 | `core` | ~72% |
| 7 | Key derivation & crypto | 0.3.0 | `crypto` | ~76% |
| 8 | Account completeness | 0.3.1 | `account`, `wallet` | ~80% |
| 9 | Contract interaction completeness | 0.3.2 | `contract` | ~85% |
| 10 | L1-L2 messaging | 0.4.0 | `ethereum` | ~87% |
| 11 | Ethereum portal managers | 0.4.1 | `ethereum` | ~91% |
| 12 | Private/public fee methods | 0.4.2 | `fee` | ~93% |
| 13 | Wallet capabilities & batching | 0.5.0 | `wallet` | ~95% |
| 14 | Protocol/block/tree/log/note types | 0.5.1 | `core`, `node-client` | ~98% |
| 15 | Utilities & polish | 0.5.2 | all | ~100% |
| 16 | Stable release | 1.0.0 | all | 100% |

---

## Dependencies Between Steps

```
Step 1 (PXE Client)
  └── Step 2 (BaseWallet)
        ├── Step 3 (Fee Payment) ─── Step 12 (Private/Public Fee)
        ├── Step 4 (AuthWit)
        ├── Step 5 (Deployment) ─── Step 8 (Account Completeness)
        └── Step 9 (Contract Completeness)

Step 6 (ABI) ── independent, can be done in parallel with 3-5
Step 7 (Crypto) ── prerequisite for Step 8

Step 10 (L1-L2 Messaging)
  └── Step 11 (Portal Managers)

Step 13 (Capabilities) ── independent
Step 14 (Types) ── independent
Step 15 (Polish) ── after all others
Step 16 (1.0) ── after Step 15
```

Steps 6, 7, 10, 13, and 14 can be parallelized with other work as they have minimal dependencies on the critical path (Steps 1 -> 2 -> 3/4/5).
