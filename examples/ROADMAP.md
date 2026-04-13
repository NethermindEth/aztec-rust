# Example Roadmap

The current `examples/` directory is stale and too broad per file. The replacement set should do two things:

1. Prefer the real happy path for Aztec v4.x in this repo: embedded PXE + local Aztec network.
2. Split workflows into small, runnable examples with one clear takeaway each.

This document is the implementation plan for the example set.

## Goals

- Every example should run against `aztec start --local-network`.
- Default to `AZTEC_NODE_URL=http://localhost:8080`.
- Use embedded PXE by default.
- Keep boilerplate in shared helpers, not duplicated in each example.
- Favor realistic flows already covered by `tests/`.
- Cover most of the workspace crates through examples, with PXE integration as the highest priority.

## Non-Goals

- Do not keep giant “do everything” examples like the current `contract_call.rs`.
- Do not require a standalone PXE RPC server.
- Do not create pure unit-style examples for plumbing crates when the behavior is already exercised better through wallet/PXE flows.

## Shared Example Harness

Before adding more examples, create a small shared helper module for `examples/`, e.g. `examples/common/mod.rs`, with:

- `node_url()` and `ethereum_url()`
- `setup_wallet(imported_account)` for a pre-funded local-network account
- `setup_wallet_with_accounts(primary, extra)` for scope/event examples
- `register_protocol_contracts()` for FeeJuice-related flows
- `register_contract_on_pxe()`
- `deploy_contract()`
- `load_*_artifact()` wrappers around `fixtures/*.json`
- `send_call()` / `build_call()`
- `read_public_u128()` / common storage-slot helpers

Design rules for the harness:

- For normal examples, prefer imported sandbox accounts over random keys.
- Use `create_embedded_wallet(...)` only for the simplest flows.
- Use direct `EmbeddedPxe::create(...)` when the example needs low-level PXE control, multiple PXEs, or seeded stores.
- Keep all env vars optional except when the flow truly needs L1 RPC (`ETHEREUM_HOST` should still default to `http://localhost:8545`).

## Replace Current Examples

Current files and what to do with them:

| Current file | Problem | Action |
| --- | --- | --- |
| `node_info.rs` | Fine conceptually, but too narrow for overall coverage | Keep, refresh output and local-network assumptions |
| `account_flow.rs` | Mostly local object construction, not enough real network behavior | Replace with a real account deployment example |
| `deploy_contract.rs` | Partial coverage only | Replace with a deploy + register + verify example |
| `contract_call.rs` | Too large, mixed concerns, still talks about missing PXE behavior | Split into focused examples below |

## Priority Order

Implementation should happen in this order:

1. Shared harness
2. PXE-first core examples
3. Account / contract / event examples
4. Fee examples
5. Cross-chain examples
6. Advanced or niche examples

## Proposed Examples

### P0: Must Have

| File | Purpose | Main crates covered | Notes |
| --- | --- | --- | --- |
| `node_info.rs` | Connect to local node, print node info, block height, rollup/L1 addresses | `node-client`, `core` | Keep this as the zero-dependency smoke test |
| `wallet_minimal.rs` | Create an embedded wallet against the local network and print chain/accounts/PXE sync state | `wallet`, `pxe`, `node-client`, `account` | This becomes the canonical bootstrap example |
| `deploy_contract.rs` | Deploy a contract, wait for registration, verify class metadata and public storage | `contract`, `deployment`, `wallet`, `pxe`, `node-client` | Use `stateful_test_contract` or `token_contract_compiled` fixture |
| `private_token_transfer.rs` | Deploy token, mint privately, transfer privately, read balances via utility calls | `contract`, `wallet`, `pxe`, `account`, `events` | This should be the main “real PXE flow” example |
| `simulate_profile_send.rs` | Run the same contract interaction through `simulate_tx`, `profile_tx`, then `send_tx` | `wallet`, `pxe`, `contract`, `fee` | Show return-value shape, gas usage, and when to use each path |
| `event_logs.rs` | Emit public + private events and fetch/decode them | `contract`, `events`, `wallet`, `pxe`, `node-client` | Use `test_log_contract_compiled.json`; include sender registration for private logs |
| `two_pxes.rs` | Two embedded PXEs, sender registration, contract registration on the second PXE, cross-wallet private state visibility | `pxe`, `wallet`, `contract`, `account` | Directly grounded in `tests/pxe/e2e_2_pxes.rs` |
| `scope_isolation.rs` | Show how scopes affect note visibility and utility/simulation results | `pxe`, `wallet`, `contract` | High-value PXE example; grounded in `tests/pxe/e2e_scope_isolation.rs` |
| `note_getter.rs` | Demonstrate note queries, status filters, and the difference between `execute_utility` and `simulate_tx` return shapes | `pxe`, `wallet`, `contract` | Grounded in `tests/pxe/e2e_note_getter.rs` |
| `account_deploy.rs` | Real `AccountManager` flow: derive address, deploy Schnorr account, then send a tx from it | `account`, `wallet`, `pxe`, `contract` | Replaces current `account_flow.rs` |

### P1: Should Have

| File | Purpose | Main crates covered | Notes |
| --- | --- | --- | --- |
| `authwit.rs` | Create an auth witness, validate it, consume it in a transaction | `account`, `contract`, `wallet`, `core` | Grounded in `tests/contract/e2e_authwit.rs` |
| `deploy_options.rs` | Show `DeployOptions`: universal deploy, skip registration, class publication behavior | `deployment`, `contract`, `wallet` | Grounded in `tests/contract/e2e_deploy_method.rs` |
| `public_storage.rs` | Public storage reads and writes with explicit slot derivation | `wallet`, `contract`, `core`, `hash` | Useful as a smaller companion to deploy/transfer examples |
| `fee_native.rs` | Native fee payment with explicit fee payload and gas settings | `fee`, `wallet`, `contract` | Smallest fee example |
| `fee_sponsored.rs` | Sponsored fee payment through a sponsor contract | `fee`, `wallet`, `contract`, `pxe` | Grounded in `tests/fee/e2e_fee_sponsored_payments.rs`; requires sponsor fixture |
| `fee_juice_claim.rs` | Bridge FeeJuice from L1, build `FeeJuicePaymentMethodWithClaim`, then spend it | `fee`, `ethereum`, `wallet`, `account`, `pxe` | Grounded in `tests/fee/e2e_fee_juice_payments.rs` |
| `l1_to_l2_message.rs` | Send an L1→L2 message, wait until ready, consume it on L2 | `ethereum`, `node-client`, `contract`, `wallet` | Grounded in `tests/ethereum/e2e_cross_chain_l1_to_l2.rs` |

### P2: Nice to Have

| File | Purpose | Main crates covered | Notes |
| --- | --- | --- | --- |
| `contract_update.rs` | Publish a new class and update a deployed contract to it | `deployment`, `contract`, `wallet`, `pxe` | Grounded in `tests/contract/e2e_contract_updates.rs` |
| `multiple_accounts_one_encryption_key.rs` | Multiple accounts sharing one encryption key with different signing keys | `account`, `wallet`, `pxe`, `crypto` | Advanced but valuable; grounded in `tests/account/e2e_multiple_accounts_1_enc_key.rs` |
| `l2_to_l1_message.rs` | Create L2→L1 messages and verify hash / outbox expectations | `ethereum`, `contract`, `wallet`, `hash` | Grounded in `tests/ethereum/e2e_cross_chain_l2_to_l1.rs` |
| `block_building.rs` | Send multiple txs and inspect resulting block/log ordering behavior | `node-client`, `wallet`, `contract` | Useful once node admin/config coverage is stronger |

## Recommended Example Style

Each example should:

- Have one primary workflow.
- Be runnable with one `cargo run --example ...` command.
- Print the minimum state needed to prove the workflow worked.
- Fail loudly on unsupported local-network state rather than silently skipping work.

Each example should not:

- Re-implement account import, artifact loading, or slot derivation inline.
- Mix deployment, events, fees, authwit, and cross-chain in one file.
- Rely on raw store mutation unless the point of the example is PXE internals.

## Suggested Fixture Usage

Use the smallest fixture that demonstrates the flow:

- `token_contract_compiled.json` for private/public token transfers
- `stateful_test_contract_compiled.json` for deploy/public-storage examples
- `test_log_contract_compiled.json` for public/private event examples
- `scope_test_contract_compiled.json` for scope isolation
- `note_getter_contract_compiled.json` for note getter behavior
- `auth_wit_test_contract_compiled.json` and `generic_proxy_contract_compiled.json` for authwit
- `updatable_contract_compiled.json` + `updated_contract_compiled.json` for contract updates

Optional fixtures from `aztec-packages` can back the more advanced fee examples when the local copies are missing.

## Crate Coverage Map

This set covers the workspace like this:

| Crate | Covered by examples |
| --- | --- |
| `aztec-rs` | All examples use umbrella re-exports |
| `aztec-core` | Node info, deploy options, public storage, authwit, messaging |
| `aztec-rpc` | Indirectly through every node/PXE flow; no dedicated example needed |
| `aztec-crypto` | Account deploy, multiple accounts one encryption key, messaging helpers |
| `aztec-node-client` | Node info, block building, cross-chain readiness, public logs |
| `aztec-pxe-client` | Wallet bootstrap, simulate/profile/send, scopes, private events |
| `aztec-pxe` | Wallet bootstrap, two PXEs, note getter, scope isolation |
| `aztec-wallet` | Nearly every example |
| `aztec-contract` | Deploy, transfers, events, authwit, updates |
| `aztec-account` | Wallet bootstrap, account deploy, authwit, multi-account examples |
| `aztec-fee` | Native, sponsored, FeeJuice claim examples |
| `aztec-ethereum` | L1→L2, L2→L1, FeeJuice claim |

## Best Sources Per Example

Use these tests as the source of truth when implementing the examples:

- `tests/common/mod.rs`
- `tests/pxe/e2e_2_pxes.rs`
- `tests/pxe/e2e_scope_isolation.rs`
- `tests/pxe/e2e_note_getter.rs`
- `tests/contract/e2e_event_logs.rs`
- `tests/contract/e2e_deploy_method.rs`
- `tests/contract/e2e_authwit.rs`
- `tests/fee/e2e_fee_juice_payments.rs`
- `tests/fee/e2e_fee_sponsored_payments.rs`
- `tests/ethereum/e2e_cross_chain_l1_to_l2.rs`
- `tests/ethereum/e2e_cross_chain_l2_to_l1.rs`
- `tests/account/e2e_multiple_accounts_1_enc_key.rs`
- `tests/contract/e2e_contract_updates.rs`

## Concrete First Batch

If we want the highest-value first pass, implement these first:

1. `examples/common/mod.rs`
2. `wallet_minimal.rs`
3. `deploy_contract.rs`
4. `private_token_transfer.rs`
5. `simulate_profile_send.rs`
6. `event_logs.rs`
7. `two_pxes.rs`
8. `scope_isolation.rs`
9. `account_deploy.rs`

That batch fixes the current stale state and establishes the PXE-centric story for the repo.
