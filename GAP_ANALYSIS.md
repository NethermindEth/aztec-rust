# aztec-rs Gap Analysis vs aztec-packages

**Date:** 2026-04-15
**Reference root:** `/Users/alexmetelli/source/aztec-packages`
**Target root:** `/Users/alexmetelli/source/aztec-rs`
**Method:** local source review only. This file intentionally avoids parity estimates that were not measured from code.

## Executive Summary

The previous version of this document was stale. It described `aztec-rs` as a single `src/*.rs` SDK with no real PXE, no `BaseWallet`, stubbed deployment, empty fee/auth/L1 modules, and no key derivation. The current repository is a multi-crate workspace and those statements are no longer accurate.

The current Rust workspace has substantial coverage:

- `crates/pxe-client` defines a `Pxe` trait covering synced header, accounts, senders, contracts, simulation, proving, profiling, utility execution, private events, and shutdown.
- `crates/pxe` implements an in-process `EmbeddedPxe` with local stores, block sync, note/event services, private execution plumbing, and kernel proving hooks.
- `crates/wallet` implements `BaseWallet<P, N, A>` over `Pxe`, `AztecNode`, and an `AccountProvider`.
- `crates/account` implements account traits, `AccountManager`, `DeployAccountMethod`, `AccountWithSecretKey`, `SignerlessAccount`, `SchnorrAccountContract`, `get_account_contract_address`, and `AccountEntrypointMetaPaymentMethod`.
- `crates/contract` implements `Contract`, `ContractFunctionInteraction`, `BatchCall`, `ContractDeployer`, `DeployMethod`, `publish_contract_class`, `publish_instance`, `get_gas_limits`, profiling, event decoding, and authwit interaction helpers.
- `crates/fee` implements `FeePaymentMethod`, `NativeFeePaymentMethod`, `SponsoredFeePaymentMethod`, and `FeeJuicePaymentMethodWithClaim`.
- `crates/ethereum` implements L1/L2 messaging types, claim secret generation, L1-to-L2 readiness helpers, and a lower-level JSON-RPC Ethereum client with Fee Juice preparation support.
- `crates/crypto` and `crates/core` implement key derivation, public key derivation, secret hashing, authwit hashing, ABI encoding/decoding/checkers, typed transaction/kernel structures, protocol constants, and node client wait helpers.

The largest current gaps are therefore not the old P0 items. The remaining gaps are mostly API parity and behavioral completeness:

1. **Wallet capability and wallet batch APIs are absent from the Rust `Wallet` trait.** TypeScript exposes `requestCapabilities()` and `batch()` in `aztec.js/src/wallet/wallet.ts` and implements them in `wallet-sdk/src/base-wallet/base_wallet.ts`. No Rust equivalent was found in `crates/wallet/src/wallet.rs`.
2. **No remote HTTP/JSON-RPC PXE client was found.** Rust has `Pxe` plus `EmbeddedPxe`, but `rg` found no `HttpPxe`, `create_pxe_client`, or JSON-RPC PXE transport implementation. Current `aztec-packages` also no longer has the old `stdlib/src/interfaces/pxe.ts` file referenced by the previous doc; the current TS PXE reference is `yarn-project/pxe/src/pxe.ts` and `pxe/src/entrypoints/client/*/utils.ts`.
3. **`PrivateFeePaymentMethod` and `PublicFeePaymentMethod` are still missing.** Rust has native, sponsored, and Fee Juice claim payment methods, but no modules or exports matching `aztec.js/src/fee/private_fee_payment_method.ts` and `public_fee_payment_method.ts`. Both are marked `@deprecated` in the upstream TS source (not supported on mainnet), which reduces urgency.
4. **Broader account package parity is incomplete.** Rust has Schnorr plus generic account abstractions and signerless support. `aztec-packages/yarn-project/accounts/src` also has ECDSA K, ECDSA R, SSH ECDSA R, single-key, and stub account contract classes.
5. **TypeScript dynamic convenience APIs are not mirrored exactly.** Rust has `Contract::method(...)`, `Contract::deploy(...)`, and `Contract::deploy_with_public_keys(...)`, but not the TS `ContractBase.methods.<name>` dynamic map or TS type-level `ContractMethod` call surface.
6. **Send/wait return-shape parity differs.** Rust node `WaitOpts` has `wait_for_status`, `dont_throw_on_revert`, and dropped-receipt grace support, but wallet `SendOptions` has no `wait`/`NO_WAIT` option and `SendResult` always contains a `tx_hash`, not the TS conditional `txHash` versus `receipt` return shape.
7. **Public API re-export parity still has gaps.** Missing or partial areas include protocol contract wrappers, block/tree/log/note public API wrappers, `readFieldCompressedString`, `getFeeJuiceBalance`, `getTimestampRangeForEpoch`, and current min-fee helpers at the wallet option level.
8. **Some PXE and node-facing data remains opaque JSON.** `crates/pxe-client/src/pxe.rs` keeps `BlockHeader`, `TxExecutionRequest`, and PXE simulation/profile payloads as `serde_json::Value` wrappers in places. `TxProvingResult::validate()` is intentionally shallow until typed public input parsing is expanded.

## Evidence Reviewed

Rust target paths:

- Workspace and re-exports: `Cargo.toml`, `src/lib.rs`
- PXE: `crates/pxe-client/src/pxe.rs`, `crates/pxe/src/embedded_pxe.rs`, `crates/pxe/src/stores/*`, `crates/pxe/src/sync/*`, `crates/pxe/src/execution/*`, `crates/pxe/src/kernel/*`
- Wallet: `crates/wallet/src/wallet.rs`, `crates/wallet/src/base_wallet.rs`, `crates/wallet/src/account_provider.rs`
- Accounts: `crates/account/src/account.rs`, `crates/account/src/schnorr.rs`, `crates/account/src/signerless.rs`, `crates/account/src/meta_payment.rs`, `crates/account/src/authorization.rs`
- Contracts/deployment/events/authwit: `crates/contract/src/contract.rs`, `crates/contract/src/deployment.rs`, `crates/contract/src/events.rs`, `crates/contract/src/authwit.rs`
- Fee: `crates/fee/src/*.rs`
- Ethereum/L1 messaging: `crates/ethereum/src/messaging.rs`, `crates/ethereum/src/cross_chain.rs`, `crates/ethereum/src/l1_client.rs`
- Core/crypto/node: `crates/core/src/abi/*`, `crates/core/src/hash.rs`, `crates/core/src/tx.rs`, `crates/core/src/kernel_types.rs`, `crates/core/src/constants.rs`, `crates/crypto/src/keys.rs`, `crates/node-client/src/node.rs`

TypeScript reference paths:

- `yarn-project/aztec.js/src/**`
- `yarn-project/wallet-sdk/src/base-wallet/base_wallet.ts`
- `yarn-project/pxe/src/pxe.ts`
- `yarn-project/pxe/src/entrypoints/client/{lazy,bundle}/utils.ts`
- `yarn-project/accounts/src/**`
- `yarn-project/stdlib/src/interfaces/aztec-node.ts`

The old reference file `yarn-project/stdlib/src/interfaces/pxe.ts` is not present in the current local `aztec-packages` tree.

## Current Module Comparison

### 1. PXE and Wallet Infrastructure

| Feature | TS reference | Rust status | Evidence |
|---|---|---|---|
| PXE public operations | `pxe/src/pxe.ts` | Implemented as `Pxe` trait | `crates/pxe-client/src/pxe.rs` |
| In-process PXE creation | `pxe/src/entrypoints/client/{bundle,lazy}/utils.ts` | Implemented as `EmbeddedPxe::create*` | `crates/pxe/src/embedded_pxe.rs` |
| PXE-backed wallet | `wallet-sdk/src/base-wallet/base_wallet.ts` | Implemented as generic `BaseWallet<P, N, A>` | `crates/wallet/src/base_wallet.rs` |
| Embedded wallet helper | `wallets/src/embedded/embedded_wallet.ts` | Implemented (feature-gated: `embedded-pxe`): `create_embedded_wallet` | `crates/wallet/src/base_wallet.rs` |
| Remote PXE HTTP client | old doc named `pxe/src/client/` | Not found | `rg "HttpPxe|create_pxe_client|createPXEClient"` found no Rust impl |
| Wallet `requestCapabilities` | `aztec.js/src/wallet/capabilities.ts`, wallet-sdk base wallet | Missing | no Rust capability types or wallet method found |
| Wallet `batch` | `aztec.js/src/wallet/wallet.ts`, wallet-sdk base wallet | Missing at wallet trait level | `BatchCall` exists for execution payloads only |
| Send `NO_WAIT` return shape | `aztec.js/src/contract/interaction_options.ts` | Missing | Rust `SendOptions` has no `wait`; `SendResult` is tx hash only |

Notes:

- The old claim "no real PXE-backed Wallet implementation" is false for the current Rust workspace.
- The old claim that `aztec.js` does not expose PXE via the current tree needs updating. Current `aztec-packages` uses an in-process `PXE` class under `yarn-project/pxe/src/pxe.ts`, with client entrypoint factories under `pxe/src/entrypoints/client`.
- Rust does not mirror TypeScript's `simulateTx` optimization for leading public static calls through `extractOptimizablePublicStaticCalls`; Rust simulation goes through PXE and then public-call simulation/preflight where needed.

### 2. Account Module

| Feature | TS reference | Rust status | Evidence |
|---|---|---|---|
| Account and auth provider traits | `aztec.js/src/account/account.ts` | Implemented | `crates/account/src/account.rs` |
| Account contract trait | `aztec.js/src/account/account_contract.ts` | Implemented | `crates/account/src/account.rs` |
| `getAccountContractAddress` | `aztec.js/src/account/account_contract.ts` | Implemented as `get_account_contract_address` | `crates/account/src/account.rs` |
| `AccountWithSecretKey` | `aztec.js/src/account/account_with_secret_key.ts` | Implemented | `crates/account/src/account.rs` |
| `SignerlessAccount` | `aztec.js/src/account/signerless_account.ts` | Implemented | `crates/account/src/signerless.rs` |
| `AccountManager` | `aztec.js/src/wallet/account_manager.ts` | Implemented | `crates/account/src/account.rs` |
| `DeployAccountMethod` | `aztec.js/src/wallet/deploy_account_method.ts` | Implemented with self-fee routing | `crates/account/src/account.rs` |
| Account meta payment method | `aztec.js/src/wallet/account_entrypoint_meta_payment_method.ts` | Implemented | `crates/account/src/meta_payment.rs` |
| Schnorr account contract | `accounts/src/schnorr/*` | Implemented | `crates/account/src/schnorr.rs` |
| ECDSA K/R, SSH ECDSA R, single-key, stub account contracts | `accounts/src/{ecdsa,single_key,stub}` | Missing | no matching Rust account contract structs found |

Notes:

- The old claims that signerless account, account address derivation, meta payment method, and account deployment were missing are no longer accurate.
- `DeployAccountMethod` defaults differ from the TS implementation in one important place: TS defaults account deployment to `skipClassPublication: true` and `skipInstancePublication: true` when not provided; Rust `DeployAccountOptions` uses the default booleans from Rust's struct unless callers set them.

### 3. Contract and Deployment Modules

| Feature | TS reference | Rust status | Evidence |
|---|---|---|---|
| `Contract.at` | `aztec.js/src/contract/contract.ts` | Implemented as `Contract::at` | `crates/contract/src/contract.rs` |
| `Contract.deploy` | `aztec.js/src/contract/contract.ts` | Implemented as `Contract::deploy` | `crates/contract/src/contract.rs` |
| `deployWithPublicKeys` | `aztec.js/src/contract/contract.ts` | Implemented as `Contract::deploy_with_public_keys` | `crates/contract/src/contract.rs` |
| Dynamic method map | `aztec.js/src/contract/contract_base.ts` | Different API | Rust uses `Contract::method(name, args)` |
| `ContractFunctionInteraction` | `aztec.js/src/contract/contract_function_interaction.ts` | Implemented | `crates/contract/src/contract.rs` |
| `BatchCall` | `aztec.js/src/contract/batch_call.ts` | Implemented | `crates/contract/src/contract.rs` |
| `profile()` on interactions and batch | `aztec.js/src/contract/contract_function_interaction.ts` | Implemented | `crates/contract/src/contract.rs` |
| `ContractDeployer` / `DeployMethod` | `aztec.js/src/deployment/contract_deployer.ts`, `contract/deploy_method.ts` | Implemented | `crates/contract/src/deployment.rs` |
| `publishContractClass` | `aztec.js/src/deployment/publish_class.ts` | Implemented | `crates/contract/src/deployment.rs` |
| `publishInstance` | `aztec.js/src/deployment/publish_instance.ts` | Implemented | `crates/contract/src/deployment.rs` |
| Address derivation from artifact/salt/deployer | stdlib contract helpers | Implemented | `get_contract_instance_from_instantiation_params` |
| `getGasLimits` | `aztec.js/src/contract/get_gas_limits.ts` | Implemented | `crates/contract/src/deployment.rs` |
| `waitForProven` | `aztec.js/src/contract/wait_for_proven.ts` | Implemented in node client | `crates/node-client/src/node.rs` |
| ABI checker | `aztec.js/src/contract/checker.ts` | Implemented as `abi_checker` | `crates/core/src/abi/checkers.rs` |
| `NO_WAIT` interaction option | `aztec.js/src/contract/interaction_options.ts` | Missing | no Rust `NO_WAIT` equivalent found |

Notes:

- The old "deployment internals are stubs" claim is false.
- Rust returns `DeployResult { send_result, instance }`, whereas TS has richer conditional return types tied to wait options.

### 4. Fee Payment

| Feature | TS reference | Rust status | Evidence |
|---|---|---|---|
| `FeePaymentMethod` | `aztec.js/src/fee/fee_payment_method.ts` | Implemented | `crates/fee/src/fee_payment_method.rs` |
| Native Fee Juice from existing balance | TS wallet default fee option behavior | Implemented as explicit `NativeFeePaymentMethod` | `crates/fee/src/native.rs` |
| `SponsoredFeePaymentMethod` | `aztec.js/src/fee/sponsored_fee_payment.ts` | Implemented | `crates/fee/src/sponsored.rs` |
| `FeeJuicePaymentMethodWithClaim` | `aztec.js/src/fee/fee_juice_payment_method_with_claim.ts` | Implemented | `crates/fee/src/fee_juice_with_claim.rs` |
| `PrivateFeePaymentMethod` | `aztec.js/src/fee/private_fee_payment_method.ts` | Missing | no Rust type/module found; upstream TS marks as `@deprecated` (not supported on mainnet) |
| `PublicFeePaymentMethod` | `aztec.js/src/fee/public_fee_payment_method.ts` | Missing | no Rust type/module found; upstream TS marks as `@deprecated` (not supported on mainnet) |

Notes:

- Both `PrivateFeePaymentMethod` and `PublicFeePaymentMethod` are marked `@deprecated` in the upstream TS source with the note "not supported on mainnet". Implementing them in Rust is therefore lower urgency; tests in `tests/fee/e2e_fee_private_payments.rs` and `tests/fee/e2e_fee_public_payments.rs` exist and mark related flows as TODO.
- Rust currently expects fee execution payloads to be pre-resolved into wallet options; it does not mirror TS's full `fee?: { paymentMethod, gasSettings, estimateGas }` shape at the wallet API boundary.

### 5. Authorization and Authwit

| Feature | TS reference | Rust status | Evidence |
|---|---|---|---|
| `CallAuthorizationRequest` | `aztec.js/src/authorization/call_authorization_request.ts` | Implemented | `crates/account/src/authorization.rs` |
| `computeAuthWitMessageHash` | `aztec.js/src/utils/authwit.ts` | Implemented as `compute_auth_wit_message_hash` | `crates/core/src/hash.rs` |
| `getMessageHashFromIntent` | `aztec.js/src/utils/authwit.ts` | Missing | generic intent-to-hash helper added alongside `computeAuthWitMessageHash`; no Rust equivalent found |
| `computeInnerAuthWitHashFromAction` | `aztec.js/src/utils/authwit.ts` | Implemented | `crates/core/src/hash.rs` |
| `lookupValidity` | `aztec.js/src/utils/authwit.ts` | Implemented | `crates/contract/src/authwit.rs` |
| `SetPublicAuthwitContractInteraction` | `aztec.js/src/utils/authwit.ts` | Implemented as `SetPublicAuthWitInteraction` | `crates/contract/src/authwit.rs` |
| Wallet `createAuthWit` | `aztec.js/src/wallet/wallet.ts` | Implemented | `crates/wallet/src/wallet.rs`, `crates/wallet/src/base_wallet.rs` |

The old claim "authorization module is empty" is false.

### 6. Ethereum and L1/L2 Messaging

| Feature | TS reference | Rust status | Evidence |
|---|---|---|---|
| `L1Actor`, `L2Actor`, `L1ToL2Message` | `aztec.js/src/api/messaging.ts` / stdlib messaging | Implemented | `crates/ethereum/src/messaging.rs` |
| `L2Claim`, `L2AmountClaim`, recipient variant | `aztec.js/src/ethereum/portal_manager.ts` | Implemented | `crates/ethereum/src/messaging.rs` |
| `generateClaimSecret` | `aztec.js/src/ethereum/portal_manager.ts` | Implemented as `generate_claim_secret` | `crates/ethereum/src/messaging.rs` |
| `isL1ToL2MessageReady` | `aztec.js/src/utils/cross_chain.ts` | Implemented | `crates/ethereum/src/cross_chain.rs` |
| `waitForL1ToL2MessageReady` | `aztec.js/src/utils/cross_chain.ts` | Implemented | `crates/ethereum/src/cross_chain.rs` |
| L1 `sendL2Message` helper | portal/inbox flows | Implemented lower-level helper | `crates/ethereum/src/l1_client.rs` |
| Fee Juice L1 preparation | portal manager flow | Partially implemented lower-level helper | `prepare_fee_juice_on_l1` |
| `L1TokenManager`, `L1FeeJuicePortalManager`, `L1ToL2TokenPortalManager`, `L1TokenPortalManager` | `aztec.js/src/ethereum/portal_manager.ts` (all four confirmed present) | Missing as class-level API | no matching Rust managers found |

The old claim "Ethereum/L1 portal integration entirely missing" is false, but the Rust API is lower-level and does not mirror the TS portal manager classes. All four TS portal manager classes (`L1TokenManager`, `L1FeeJuicePortalManager`, `L1ToL2TokenPortalManager`, `L1TokenPortalManager`) are confirmed present in `portal_manager.ts`.

### 7. Node Client

| Feature | TS reference | Rust status | Evidence |
|---|---|---|---|
| `createAztecNodeClient` | `aztec.js/src/utils/node.ts` | Implemented | `crates/node-client/src/node.rs` |
| `waitForNode` | `aztec.js/src/utils/node.ts` | Implemented | `crates/node-client/src/node.rs` |
| `waitForTx` | `aztec.js/src/utils/node.ts` | Implemented | `crates/node-client/src/node.rs` |
| `WaitOpts` `waitForStatus` | `aztec.js/src/contract/wait_opts.ts` | Implemented as `wait_for_status` | `crates/node-client/src/node.rs` |
| `WaitOpts` `dontThrowOnRevert` | `aztec.js/src/contract/wait_opts.ts` | Implemented as `dont_throw_on_revert` | `crates/node-client/src/node.rs` |
| `WaitOpts` dropped receipt grace | `aztec.js/src/contract/wait_opts.ts` | Implemented as `ignore_dropped_receipts_for` | `crates/node-client/src/node.rs` |
| Expanded Aztec node methods for PXE | `stdlib/src/interfaces/aztec-node.ts` | Partially implemented | `crates/node-client/src/node.rs` |

The old claim that `WaitOpts` lacked the advanced fields is false.

### 8. ABI, Types, Crypto, and Transactions

| Feature | TS reference | Rust status | Evidence |
|---|---|---|---|
| `Fr`, `Fq`, `Point`, `GrumpkinScalar`, addresses, public keys | `aztec.js/src/api/{fields,addresses,keys}.ts` | Implemented/re-exported | `crates/core/src/types.rs`, `src/lib.rs` |
| `FunctionSelector`, `EventSelector`, `NoteSelector` | `aztec.js/src/api/abi.ts` | Implemented | `crates/core/src/abi/selectors.rs` |
| `FunctionSelector` derivation from signature/name+params | stdlib ABI | Implemented | `crates/core/src/abi/selectors.rs` |
| `ContractArtifact`, `FunctionArtifact`, `FunctionType` | `aztec.js/src/api/abi.ts` | Implemented | `crates/core/src/abi/types.rs` |
| `encodeArguments` | `aztec.js/src/api/abi.ts` / stdlib | Implemented as `encode_arguments` | `crates/core/src/abi/encoder.rs` |
| `decodeFromAbi` | `aztec.js/src/api/abi.ts` / stdlib | Implemented as `decode_from_abi` | `crates/core/src/abi/decoder.rs` |
| ABI type checkers | `aztec.js/src/utils/abi_types.ts`, checker.ts | Partially implemented | `crates/core/src/abi/checkers.rs` |
| Contract storage layout type | `aztec.js/src/contract/contract_base.ts` | Implemented | `crates/core/src/abi/storage_layout.rs` |
| Key derivation | `aztec.js/src/api/keys.ts` / stdlib keys | Implemented | `crates/crypto/src/keys.rs` |
| `generatePublicKey` | `aztec.js/src/utils/pub_key.ts` | Implemented as `derive_public_key_from_secret_key` | `crates/crypto/src/keys.rs` |
| `computeSecretHash` | `aztec.js/src/api/crypto.ts` | Implemented | `crates/core/src/hash.rs`, re-exported by `crates/crypto` |
| `Tx`, `TypedTx`, `TxReceipt`, `TxStatus`, `ExecutionPayload`, `mergeExecutionPayloads` | `aztec.js/src/api/tx.ts` | Implemented in Rust shape | `crates/core/src/tx.rs` |
| `GlobalVariables` and kernel structs | stdlib tx/kernel | Implemented | `crates/core/src/kernel_types.rs` |

Remaining gaps in this area:

- TS `FieldLike`, `AztecAddressLike`, `EthAddressLike`, `FunctionSelectorLike`, `OptionLike<T>`, and other JS ergonomic input unions do not have direct Rust equivalents. Rust uses typed `AbiValue` and concrete Rust types.
- Contract artifact buffer helpers (`contractArtifactToBuffer`, `contractArtifactFromBuffer`) were not found as Rust public APIs.
- `readFieldCompressedString` was not found.

### 9. Events, Logs, Notes, Blocks, Trees, and Protocol Exports

| Feature | TS reference | Rust status | Evidence |
|---|---|---|---|
| Public event types and decoder | `aztec.js/src/api/events.ts` | Implemented | `crates/contract/src/events.rs` |
| Private event filter/result path | `aztec.js/src/wallet/wallet.ts` | Implemented | `crates/wallet/src/wallet.rs`, `crates/pxe-client/src/pxe.rs` |
| `LogId` / public log filters | `aztec.js/src/api/log.ts` / stdlib logs | Partially implemented | `crates/node-client/src/node.rs` |
| Stored notes and note filtering internals | `aztec.js/src/api/note.ts` / PXE note services | Implemented internally, not aztec.js-style public API | `crates/pxe/src/stores/note_store.rs`, `crates/pxe/src/execution/pick_notes.rs` |
| `L2Block`, `Body` | `aztec.js/src/api/block.ts` (re-exports from `@aztec/stdlib/block`) | Missing as direct public types | no matching Rust `L2Block`/`Body` types found |
| `getTimestampRangeForEpoch` | `aztec.js/src/api/block.ts` (re-exports from `@aztec/stdlib/epoch-helpers`) | Missing | no Rust public helper found |
| `SiblingPath`, `MerkleTreeId` public API | `aztec.js/src/api/trees.ts` (re-exports from `@aztec/foundation/trees` and `@aztec/stdlib/trees`) | Missing as direct public API | node methods accept string/tree ids internally |
| Protocol contract addresses | `aztec.js/src/api/protocol.ts` | Partially implemented as functions/constants | `crates/core/src/constants.rs` |
| Protocol contract wrapper classes | `aztec.js/src/api/protocol.ts` | Missing | no Rust wrappers found |

Notes:

- `aztec.js/src/api/protocol.ts` imports generated files under `src/contract/protocol_contracts/*`, but those generated files were not present in the checked-out local source tree during review. Rust has protocol addresses and direct interactions, not generated wrapper classes.

## Priority Roadmap

### P0 - Real parity blockers for application/wallet APIs

1. Add wallet capabilities surface:
   - `CAPABILITY_VERSION`
   - capability structs/enums matching `aztec.js/src/wallet/capabilities.ts`
   - `Wallet::request_capabilities(...)`
   - default `BaseWallet` behavior equivalent to TS "not implemented" or an explicit unsupported result

2. Add wallet batch method parity:
   - `BatchedMethod` / `BatchResults`-style Rust API if useful
   - or document that Rust only supports execution-payload batching through `BatchCall`

3. Decide `PublicFeePaymentMethod` and `PrivateFeePaymentMethod` stance:
   - Both are marked `@deprecated` in upstream TS and are not supported on mainnet
   - If testnet/devnet FPC flows are a near-term need, implement them; otherwise defer to P2 until mainnet support lands upstream
   - Reference: `aztec.js/src/fee/public_fee_payment_method.ts`, `aztec.js/src/fee/private_fee_payment_method.ts`
   - Test stubs exist in `tests/fee/e2e_fee_private_payments.rs` and `tests/fee/e2e_fee_public_payments.rs`

4. Decide the remote PXE stance:
   - if Rust should support remote PXE, add an HTTP JSON-RPC `Pxe` implementation
   - if Rust intentionally only supports embedded PXE for Aztec v4, update docs and remove references to the old `pxe/src/client/` and `stdlib/src/interfaces/pxe.ts` paths

### P1 - Account and wallet feature completeness

5. Add broader `@aztec/accounts` account flavors:
   - ECDSA K
   - ECDSA R
   - SSH ECDSA R
   - single-key
   - stub/testing account contract helpers

6. Align account deployment defaults with TS where intended:
   - TS `DeployAccountMethod.request()` defaults `skipClassPublication` and `skipInstancePublication` to `true`
   - Rust currently inherits struct defaults unless callers set options

7. Add wallet send wait options if TS parity is required:
   - `NO_WAIT` equivalent or idiomatic Rust enum
   - send result variant for receipt versus hash
   - propagation of `WaitOpts` into wallet send

### P2 - Public API completeness

8. Add convenience wrappers/utilities:
   - `get_fee_juice_balance`
   - `read_field_compressed_string`
   - `get_timestamp_range_for_epoch`
   - `get_message_hash_from_intent` (generic multi-intent-type authwit hash helper; upstream `getMessageHashFromIntent` accepts `Fr | IntentInnerHash | CallIntent | ContractFunctionInteractionCallIntent`)
   - contract artifact buffer serialization helpers

9. Add public block/tree/log/note API wrappers:
   - `L2Block`, `Body`
   - `SiblingPath`, `MerkleTreeId`
   - `Comparator`, `Note`
   - public `LogFilter` equivalents where currently only node/PXE internals exist

10. Add protocol wrapper surface:
   - `ProtocolContractAddress`-style enum/wrapper if desired
   - generated or hand-written protocol contract handles for auth registry, contract class registry, contract instance registry, Fee Juice, multicall entrypoint, public checks

### P3 - Behavioral and type tightening

11. Replace opaque PXE JSON wrappers with typed structs where practical:
   - `BlockHeader`
   - `TxExecutionRequest`
   - PXE simulation/profile result data
   - block and tx effect responses

12. Expand `TxProvingResult::validate()`:
   - parse typed public inputs
   - verify calldata/log count invariants instead of only carrying placeholders

13. Consider porting TS wallet simulation optimization:
   - `extractOptimizablePublicStaticCalls`
   - `simulateViaNode`
   - `buildMergedSimulationResult`

## Removed Stale Items

These old gap statements should not be reintroduced unless the code regresses:

- "No real PXE-backed Wallet implementation" - false; see `BaseWallet` and `EmbeddedPxe`.
- "PXE trait missing" - false; see `crates/pxe-client/src/pxe.rs`.
- "Fee payment strategies only have types" - false; native, sponsored, and Fee Juice claim strategies exist.
- "Authorization module empty" - false; auth request and authwit helpers exist.
- "L1-L2 messaging module empty" - false; messaging and readiness helpers exist.
- "Ethereum/L1 portal integration entirely missing" - false; lower-level L1 client and Fee Juice helper exist.
- "Deployment internals are stubs" - false; class and instance publication interactions are implemented.
- "`getGasLimits` missing" - false; implemented as `get_gas_limits`.
- "`waitForProven` missing" - false; implemented as `wait_for_proven`.
- "Signerless account missing" - false; implemented.
- "Key derivation missing" - false; implemented in `crates/crypto/src/keys.rs`.
- "`FunctionSelector.from_name` stub" - false; Rust has `from_signature` and `from_name_and_parameters`.
- "ABI encode/decode/checkers missing" - false; Rust has encoder, decoder, and checker modules.
